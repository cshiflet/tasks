//! Port of `TaskListQueryNonRecursive.getNonRecursiveQuery`.
//!
//! A simpler sibling to the recursive builder — used for
//! `RecentlyModifiedFilter`, `AstridOrderingFilter` when the Astrid-manual-
//! order preference is on, and as the fallback when a filter declares it
//! doesn't support sorting or subtasks.
//!
//! Shape: `JOINS <filter.sql> GROUP BY tasks._id ORDER BY <sort>` with
//! optional `completedTasksAtBottom` prelude on ORDER BY, and a LIMIT
//! suffix. The recursive CTE is not used, so subtasks come out flat.

use crate::query::filter::QueryFilter;
use crate::query::permasql;
use crate::query::preferences::QueryPreferences;
use crate::query::sort;

/// Mirrors `TaskListQueryNonRecursive.JOINS` — the CalDAV + geofence +
/// place LEFT JOINs from the recursive query, plus a LEFT JOIN to `tags`
/// aliased as `for_tags` so tag_uid group_concat can be selected per task.
const JOINS: &str = r#"
LEFT JOIN tags AS for_tags ON tasks._id = for_tags.task
LEFT JOIN caldav_tasks AS for_caldav ON tasks._id = for_caldav.cd_task AND for_caldav.cd_deleted = 0
LEFT JOIN caldav_lists ON for_caldav.cd_calendar = caldav_lists.cdl_uuid
LEFT JOIN caldav_accounts ON caldav_lists.cdl_account = caldav_accounts.cda_uuid
LEFT JOIN geofences ON geofences.task = tasks._id
LEFT JOIN places ON places.uid = geofences.place
"#;

/// Mirrors `TaskListQueryNonRecursive.FIELDS`: everything the recursive
/// query selects plus the per-task tag concat and `parentComplete`.
const SELECT_FIELDS: &str = "tasks.*, \
     for_caldav.*, \
     caldav_accounts.cda_account_type AS accountType, \
     geofences.*, \
     places.*, \
     group_concat(distinct(for_tags.tag_uid)) AS tags, \
     tasks.completed > 0 AS parentComplete";

pub fn build_non_recursive_query(
    filter: &QueryFilter,
    prefs: &QueryPreferences,
    now_ms: i64,
    limit: Option<usize>,
) -> String {
    let filter_sql = match filter {
        QueryFilter::Custom { sql, .. } => permasql::replace_placeholders_for_query(sql, now_ms),
        QueryFilter::Caldav { .. } => {
            // CalDAV filters never take the non-recursive path in upstream;
            // included for completeness so the type signature is total.
            String::new()
        }
    };

    let is_recently_modified = matches!(
        filter,
        QueryFilter::Custom {
            is_recently_modified: true,
            ..
        }
    );

    let joined = format!("{JOINS}{filter_sql}");
    let sort_group_expr = sort::get_sort_group(prefs.group_mode).unwrap_or("NULL");
    let sorted = sort::adjust_query_for_flags_and_sort(prefs, joined, prefs.sort_mode);

    // Only prepend the completed-at-bottom sort prelude when the flag is
    // on; an empty prelude would be a harmless double-space in the SQL
    // but masks future mistakes where only one half is populated.
    let order_prelude = if prefs.completed_tasks_at_bottom {
        "parentComplete ASC, tasks.completed DESC, "
    } else {
        ""
    };

    // Mirrors the `groupedQuery` switch: where to wedge GROUP BY. The
    // Android version is case-sensitive on both `contains` and `replace`,
    // so we follow the same rule — filter SQL that happens to spell the
    // keyword lowercase would diverge from upstream anyway, which is a
    // symptom worth surfacing rather than silently papering over.
    let grouped = if is_recently_modified {
        sorted.replace("ORDER BY", "GROUP BY tasks._id ORDER BY")
    } else if sorted.contains("ORDER BY") {
        sorted.replacen(
            "ORDER BY",
            &format!("GROUP BY tasks._id ORDER BY {order_prelude}"),
            1,
        )
    } else if prefs.completed_tasks_at_bottom {
        format!("{sorted} GROUP BY tasks._id ORDER BY {order_prelude}")
    } else {
        format!("{sorted} GROUP BY tasks._id")
    };

    let head = format!("SELECT {SELECT_FIELDS}, {sort_group_expr} AS sortGroup FROM tasks");
    let limit_suffix = match limit {
        Some(n) => format!(" LIMIT {n}"),
        None => String::new(),
    };

    format!("{head} {grouped}{limit_suffix}")
}
