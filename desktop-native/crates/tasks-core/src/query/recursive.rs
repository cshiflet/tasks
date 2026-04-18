//! Port of `TaskListQueryRecursive.getRecursiveQuery` +
//! `TaskListQuery.getQuery`.
//!
//! Builds the `WITH RECURSIVE recursive_tasks AS ...` query the Android
//! client uses to materialise a task list with subtask indent, collapse
//! state, primary/secondary sort, and optional group headers.
//!
//! The output is a plain SQL string; callers prepare it against the same
//! SQLite connection the rest of tasks-core uses. The read-only desktop
//! client never mutates the DB, so the Android-side query rewriting
//! concerns (showCompleted / showHidden) are implemented but passing
//! `false` for both is the default and most common path.

use crate::query::filter::QueryFilter;
use crate::query::permasql;
use crate::query::preferences::QueryPreferences;
use crate::query::sort;

/// Predicate: task is not completed, not deleted, past its hide-until.
/// Mirrors `TaskDao.TaskCriteria.activeAndVisible`.
const ACTIVE_AND_VISIBLE: &str =
    "tasks.completed<=0 AND tasks.deleted<=0 AND tasks.hideUntil<=(strftime('%s','now')*1000)";

/// Mirrors `TaskListQuery.JOINS`. LEFT joins to surface CalDAV metadata,
/// geofence, and place rows alongside each task so a single pass can render
/// list/account/location details without an N+1.
const OUTER_JOINS: &str = r#"
LEFT JOIN caldav_tasks AS for_caldav ON tasks._id = for_caldav.cd_task AND for_caldav.cd_deleted = 0
LEFT JOIN caldav_lists ON for_caldav.cd_calendar = caldav_lists.cdl_uuid
LEFT JOIN caldav_accounts ON caldav_lists.cdl_account = caldav_accounts.cda_uuid
LEFT JOIN geofences ON geofences.task = tasks._id
LEFT JOIN places ON places.uid = geofences.place
"#;

/// Mirrors `TaskListQuery.FIELDS`.
const SELECT_FIELDS: &str = "tasks.*, \
     for_caldav.*, \
     caldav_accounts.cda_account_type AS accountType, \
     geofences.*, \
     places.*";

/// Builds the recursive task list query.
///
/// `now_ms` is used to expand `PermaSql` placeholders (`NOW()`, `EOD()`, …)
/// inside custom filter SQL. For `Caldav` filters it is unused; pass the
/// current wall clock in milliseconds either way.
pub fn build_recursive_query(
    filter: &QueryFilter,
    preferences: &QueryPreferences,
    now_ms: i64,
    limit: Option<usize>,
) -> String {
    let parent_query = match filter {
        QueryFilter::Caldav { calendar_uuid, .. } => caldav_parent_query(calendar_uuid),
        QueryFilter::Custom { sql, .. } => permasql::replace_placeholders_for_query(sql, now_ms),
    };

    let manual_sort = preferences.is_manual_sort;
    let group_preference = preferences.group_mode;
    let is_caldav_filter = matches!(filter, QueryFilter::Caldav { .. });

    // When a CalDAV filter is combined with manual sort or list-grouping,
    // grouping collapses to none so the server-defined order wins.
    let group_mode = if is_caldav_filter && (manual_sort || group_preference == sort::SORT_LIST) {
        sort::GROUP_NONE
    } else {
        group_preference
    };

    let sort_mode = if !manual_sort || !is_caldav_filter {
        preferences.sort_mode
    } else if let QueryFilter::Caldav {
        is_google_tasks: true,
        ..
    } = filter
    {
        sort::SORT_GTASKS
    } else {
        sort::SORT_CALDAV
    };

    let subtask_preference = preferences.subtask_mode;
    let subtask_mode = if sort_mode == sort::SORT_GTASKS || sort_mode == sort::SORT_CALDAV {
        sort_mode
    } else if subtask_preference == sort::SORT_MANUAL {
        sort::SORT_CALDAV
    } else {
        subtask_preference
    };

    let group_ascending = preferences.group_ascending && group_mode != sort::GROUP_NONE;
    let sort_ascending = preferences.sort_ascending
        && sort_mode != sort::SORT_GTASKS
        && sort_mode != sort::SORT_CALDAV;
    let subtask_ascending = preferences.subtask_ascending
        && subtask_mode != sort::SORT_GTASKS
        && subtask_mode != sort::SORT_CALDAV;

    let completed_at_bottom = preferences.completed_tasks_at_bottom;
    let parent_completed = if completed_at_bottom {
        "tasks.completed > 0"
    } else {
        "0"
    };
    let completion_sort_expr = if completed_at_bottom {
        let inner = sort::order_select_for_sort_type_recursive(preferences.completed_mode, false);
        format!("(CASE WHEN tasks.completed > 0 THEN {inner} ELSE 0 END)")
    } else {
        "0".to_string()
    };

    let primary_group_expr = sort::order_select_for_sort_type_recursive(group_mode, true);
    let primary_sort_expr = sort::order_select_for_sort_type_recursive(sort_mode, false);
    let secondary_sort_expr = sort::order_select_for_sort_type_recursive(subtask_mode, false);

    let sort_group = sort::get_sort_group(group_mode).unwrap_or("null");

    let list_join = if group_mode == sort::SORT_LIST {
        "INNER JOIN caldav_tasks ct_group ON ct_group.cd_task = tasks._id AND ct_group.cd_deleted = 0\n\
         INNER JOIN caldav_lists ON ct_group.cd_calendar = cdl_uuid"
    } else {
        ""
    };

    let completion_sort_direction = if preferences.completed_ascending {
        ""
    } else {
        " DESC"
    };

    let group_order = sort::order_for_group_type_recursive(group_mode, group_ascending);
    let sort_order = sort::order_for_sort_type_recursive(
        sort_mode,
        sort_ascending,
        subtask_mode,
        subtask_ascending,
    );

    let limit_clause = match limit {
        Some(n) => format!("LIMIT {n}"),
        None => String::new(),
    };

    let query = format!(
        r#"WITH RECURSIVE recursive_tasks AS (
    SELECT
        tasks._id AS task,
        {parent_completed} AS parent_complete,
        {completion_sort_expr} AS completion_sort,
        0 AS parent,
        tasks.collapsed AS collapsed,
        0 AS hidden,
        0 AS indent,
        UPPER(tasks.title) AS sort_title,
        {primary_group_expr} AS primary_group,
        {primary_sort_expr} AS primary_sort,
        NULL AS secondary_sort,
        {sort_group} AS sort_group,
        '/' || tasks._id || '/' AS recursive_path
    FROM tasks
    {list_join}
    {parent_query}
    UNION ALL SELECT
        tasks._id AS task,
        {parent_completed} AS parent_complete,
        {completion_sort_expr} AS completion_sort,
        recursive_tasks.task AS parent,
        tasks.collapsed AS collapsed,
        CASE WHEN recursive_tasks.collapsed > 0 OR recursive_tasks.hidden > 0 THEN 1 ELSE 0 END AS hidden,
        CASE
            WHEN {parent_completed} AND recursive_tasks.parent_complete = 0 THEN 0
            ELSE recursive_tasks.indent + 1
        END AS indent,
        UPPER(tasks.title) AS sort_title,
        recursive_tasks.primary_group AS primary_group,
        recursive_tasks.primary_sort AS primary_sort,
        {secondary_sort_expr} AS secondary_sort,
        recursive_tasks.sort_group AS sort_group,
        recursive_tasks.recursive_path || tasks._id || '/' AS recursive_path
    FROM tasks
    INNER JOIN recursive_tasks ON tasks.parent = recursive_tasks.task
    WHERE
        {ACTIVE_AND_VISIBLE}
        AND recursive_tasks.recursive_path NOT LIKE '%/' || tasks._id || '/%'
    ORDER BY
        parent_complete,
        indent DESC,
        completion_sort{completion_sort_direction},
        {group_order},
        {sort_order}
),
max_indent AS (
    SELECT
        *,
        MAX(recursive_tasks.indent) OVER (PARTITION BY task) AS max_indent,
        ROW_NUMBER() OVER () AS sequence
    FROM recursive_tasks
),
descendants_recursive AS (
    SELECT
        parent,
        task AS descendant,
        parent_complete AS completed
    FROM recursive_tasks
    WHERE parent > 0
    UNION
    SELECT
        d.parent,
        r.task AS descendant,
        r.parent_complete AS completed
    FROM recursive_tasks r
    JOIN descendants_recursive d ON r.parent = d.descendant
),
descendants AS (
    SELECT
        parent,
        COUNT(DISTINCT CASE WHEN completed > 0 THEN descendant ELSE NULL END) AS completed_children,
        COUNT(DISTINCT CASE WHEN completed = 0 THEN descendant ELSE NULL END) AS uncompleted_children
    FROM descendants_recursive
    GROUP BY parent
)
SELECT
    {SELECT_FIELDS},
    group_concat(distinct(tag_uid)) AS tags,
    indent,
    sort_group,
    CASE
        WHEN parent_complete > 0 THEN completed_children
        ELSE uncompleted_children
    END AS children,
    primary_sort,
    secondary_sort,
    parent_complete
FROM tasks
    INNER JOIN max_indent
        ON tasks._id = max_indent.task
        AND indent = max_indent
        AND hidden = 0
    LEFT JOIN descendants ON descendants.parent = tasks._id
    LEFT JOIN tags ON tags.task = tasks._id
    {OUTER_JOINS}
GROUP BY tasks._id
ORDER BY sequence
{limit_clause}"#,
    );

    sort::adjust_query_for_flags(preferences, query)
}

/// Mirrors `TaskListQueryRecursive.newCaldavQuery(list)`. The uuid is
/// embedded as a single-quoted literal; it always comes from an internal
/// `CaldavCalendar.uuid` field, never user-controlled free text.
fn caldav_parent_query(calendar_uuid: &str) -> String {
    let escaped = calendar_uuid.replace('\'', "''");
    format!(
        "INNER JOIN caldav_tasks ON caldav_tasks.cd_calendar = '{escaped}' \
         AND caldav_tasks.cd_task = tasks._id AND caldav_tasks.cd_deleted = 0 \
         WHERE {ACTIVE_AND_VISIBLE} AND tasks.parent = 0"
    )
}
