//! Port of `kmp/.../com/todoroo/astrid/core/SortHelper.kt`.
//!
//! Emits the SQL fragments the recursive task-list query embeds for sorting
//! and grouping. The constants must stay in lockstep with the Kotlin ones
//! because filters persisted by the Android client store these numeric
//! sort-mode identifiers in user preferences.

use crate::query::preferences::QueryPreferences;

pub const GROUP_NONE: i32 = -1;
pub const SORT_AUTO: i32 = 0;
pub const SORT_ALPHA: i32 = 1;
pub const SORT_DUE: i32 = 2;
pub const SORT_IMPORTANCE: i32 = 3;
pub const SORT_MODIFIED: i32 = 4;
pub const SORT_CREATED: i32 = 5;
pub const SORT_GTASKS: i32 = 6;
pub const SORT_CALDAV: i32 = 7;
pub const SORT_START: i32 = 8;
pub const SORT_LIST: i32 = 9;
pub const SORT_COMPLETED: i32 = 10;
pub const SORT_MANUAL: i32 = 11;

/// Matches `org.tasks.data.dao.APPLE_EPOCH` — seconds between the Unix epoch
/// (1970-01-01) and the Apple/CalDAV epoch (2001-01-01). Used by the CalDAV
/// sort-order column expression.
const APPLE_EPOCH: i64 = 978_307_200_000;

const NO_DATE: i64 = 3_538_339_200_000;

const ADJUSTED_DUE_DATE: &str =
    "(CASE WHEN (dueDate / 1000) % 60 > 0 THEN dueDate ELSE (dueDate + 43140000) END)";
const ADJUSTED_START_DATE: &str =
    "(CASE WHEN (hideUntil / 1000) % 60 > 0 THEN hideUntil ELSE (hideUntil + 86399000) END)";

fn caldav_order_column() -> String {
    format!("IFNULL(tasks.`order`, (tasks.created - {APPLE_EPOCH}) / 1000)")
}

fn group_due_date() -> String {
    format!("((CASE WHEN (tasks.dueDate=0) THEN {NO_DATE} ELSE tasks.dueDate END)+tasks.importance * 1000)")
}

fn sort_due_date() -> String {
    let adjusted = ADJUSTED_DUE_DATE.replace("dueDate", "tasks.dueDate");
    format!("((CASE WHEN (tasks.dueDate=0) THEN {NO_DATE} ELSE {adjusted} END)+tasks.importance * 1000)")
}

fn group_start_date() -> String {
    format!("((CASE WHEN (tasks.hideUntil=0) THEN {NO_DATE} ELSE tasks.hideUntil END)+tasks.importance * 1000)")
}

fn sort_start_date() -> String {
    let adjusted = ADJUSTED_START_DATE.replace("hideUntil", "tasks.hideUntil");
    format!("((CASE WHEN (tasks.hideUntil=0) THEN {NO_DATE} ELSE {adjusted} END)+tasks.importance * 1000)")
}

fn sort_group_expression(column: &str) -> String {
    format!("datetime({column} / 1000, 'unixepoch', 'localtime', 'start of day')")
}

/// Mirrors `SortHelper.getSortGroup`.
pub fn get_sort_group(sort_type: i32) -> Option<&'static str> {
    match sort_type {
        SORT_DUE => Some("tasks.dueDate"),
        SORT_START => Some("tasks.hideUntil"),
        SORT_IMPORTANCE => Some("tasks.importance"),
        SORT_MODIFIED => Some("tasks.modified"),
        SORT_CREATED => Some("tasks.created"),
        SORT_LIST => Some("cdl_id"),
        _ => None,
    }
}

/// Mirrors `SortHelper.orderSelectForSortTypeRecursive`.
pub fn order_select_for_sort_type_recursive(sort_type: i32, grouping: bool) -> String {
    let fallback_due = || {
        let adjusted = ADJUSTED_DUE_DATE.replace("dueDate", "tasks.dueDate");
        format!(
            "(CASE WHEN (tasks.dueDate=0) THEN (strftime('%s','now')*1000)*2 ELSE ({adjusted}) END) + 172799999 * tasks.importance"
        )
    };

    match sort_type {
        GROUP_NONE => "1".to_string(),
        SORT_ALPHA => "UPPER(tasks.title)".to_string(),
        SORT_DUE => {
            if grouping {
                sort_group_expression(&group_due_date())
            } else {
                sort_due_date()
            }
        }
        SORT_START => {
            if grouping {
                sort_group_expression(&group_start_date())
            } else {
                sort_start_date()
            }
        }
        SORT_IMPORTANCE => "tasks.importance".to_string(),
        SORT_MODIFIED => {
            if grouping {
                sort_group_expression("tasks.modified")
            } else {
                "tasks.modified".to_string()
            }
        }
        SORT_CREATED => {
            if grouping {
                sort_group_expression("tasks.created")
            } else {
                "tasks.created".to_string()
            }
        }
        SORT_GTASKS => "tasks.`order`".to_string(),
        SORT_CALDAV => caldav_order_column(),
        SORT_LIST => "CASE WHEN cdl_order = -1 THEN cdl_name ELSE cdl_order END".to_string(),
        SORT_COMPLETED => "tasks.completed".to_string(),
        _ => fallback_due(),
    }
}

/// Mirrors `SortHelper.orderForGroupTypeRecursive`.
pub fn order_for_group_type_recursive(_group_mode: i32, ascending: bool) -> String {
    if ascending {
        "primary_group ASC".to_string()
    } else {
        "primary_group DESC".to_string()
    }
}

/// Mirrors `SortHelper.orderForSortTypeRecursive`.
pub fn order_for_sort_type_recursive(
    sort_mode: i32,
    primary_ascending: bool,
    secondary_mode: i32,
    secondary_ascending: bool,
) -> String {
    let primary_dir = if primary_ascending || sort_mode == SORT_GTASKS || sort_mode == SORT_CALDAV {
        "ASC"
    } else {
        "DESC"
    };
    let secondary_dir =
        if secondary_ascending || secondary_mode == SORT_GTASKS || secondary_mode == SORT_CALDAV {
            "ASC"
        } else {
            "DESC"
        };
    let mut clause = format!("primary_sort {primary_dir}, secondary_sort {secondary_dir}");
    if sort_mode != SORT_ALPHA {
        clause.push_str(", sort_title ASC");
    }
    clause
}

/// Mirrors `QueryUtils.showHidden`: rewrites the active-visible predicate
/// `tasks.hideUntil<=(strftime('%s','now')*1000)` to `1` so hidden tasks
/// appear.
///
/// The upstream Kotlin regex is `tasks\.hideUntil<=?(strftime\('%s','now'\)\*1000)`
/// — the `<=?` branch matches `<` or `<=`, but every SQL emitter in this
/// module only produces `<=`, so we can keep a plain `str::replace` with
/// the exact literal rather than pulling in a regex crate for a dormant
/// alternative.
pub fn show_hidden(sql: &str) -> String {
    sql.replace("tasks.hideUntil<=(strftime('%s','now')*1000)", "1")
}

/// Mirrors `QueryUtils.showCompleted`: rewrites `tasks.completed<=0`
/// or `tasks.completed=0` to `1` so completed tasks appear. The upstream
/// regex is case-sensitive and space-sensitive and so is this.
pub fn show_completed(sql: &str) -> String {
    sql.replace("tasks.completed<=0", "1")
        .replace("tasks.completed=0", "1")
}

/// Mirrors `SortHelper.adjustQueryForFlags`. Applied after the SQL is built
/// so the show-hidden/show-completed toggles can flip the predicate.
pub fn adjust_query_for_flags(prefs: &QueryPreferences, sql: String) -> String {
    let mut adjusted = sql;
    if prefs.show_completed {
        adjusted = show_completed(&adjusted);
    }
    if prefs.show_hidden {
        adjusted = show_hidden(&adjusted);
    }
    adjusted
}

/// Non-recursive ORDER BY expression for a given sort type. Mirrors
/// `SortHelper.orderForSortType` (the private companion of
/// `orderForSortTypeRecursive`) — uses bare column names (no `tasks.` prefix)
/// because the non-recursive query has `tasks.*` selected directly rather
/// than aliased through a CTE.
///
/// Returns the raw expression *without* a direction suffix; the caller pairs
/// it with `ASC` or `DESC` based on `sort_ascending` and the semantics of
/// the particular sort type (`SORT_MODIFIED`/`SORT_CREATED` default to DESC).
pub fn order_expr_for_sort_type(sort_type: i32) -> String {
    let fallback = || {
        format!(
            "(CASE WHEN (dueDate=0) THEN (strftime('%s','now')*1000)*2 ELSE ({ADJUSTED_DUE_DATE}) END) + 172799999 * importance"
        )
    };
    match sort_type {
        SORT_ALPHA => "UPPER(title)".to_string(),
        SORT_DUE => format!(
            "(CASE WHEN (dueDate=0) THEN (strftime('%s','now')*1000)*2 ELSE {ADJUSTED_DUE_DATE} END)+importance"
        ),
        SORT_START => format!(
            "(CASE WHEN (hideUntil=0) THEN (strftime('%s','now')*1000)*2 ELSE {ADJUSTED_START_DATE} END)+importance"
        ),
        SORT_IMPORTANCE => "importance".to_string(),
        SORT_MODIFIED => "tasks.modified".to_string(),
        SORT_CREATED => "tasks.created".to_string(),
        // Mirrors `SortHelper.ORDER_LIST`: primary is `UPPER(cdl_order)`;
        // the secondary (`cdl_name ASC`) is appended unconditionally in
        // `adjust_query_for_flags_and_sort` so direction flips affect the
        // primary only, matching Kotlin's `Order.reverse()` semantics.
        SORT_LIST => "UPPER(cdl_order)".to_string(),
        _ => fallback(),
    }
}

/// Mirrors `SortHelper.adjustQueryForFlagsAndSort`: appends an ORDER BY
/// clause to `sql` if one isn't already present, then applies the
/// show-hidden/show-completed rewrites.
///
/// Direction semantics: Kotlin's `reverse()`-if-disagrees dance cancels
/// out to "final direction = `preferences.sortAscending`" regardless of
/// the sort type's natural direction, because each `orderForSortType`
/// branch picks its natural `asc`/`desc` first and then only flips when
/// it disagrees with the preference. We collapse the dance here.
pub fn adjust_query_for_flags_and_sort(
    prefs: &QueryPreferences,
    sql: String,
    sort_type: i32,
) -> String {
    let sql = if !sql.to_uppercase().contains("ORDER BY") {
        let expr = order_expr_for_sort_type(sort_type);
        let dir = if prefs.sort_ascending { "ASC" } else { "DESC" };
        let with_secondary = match sort_type {
            // ALPHA has no secondary; its primary IS `UPPER(title)`.
            SORT_ALPHA => format!("{expr} {dir}"),
            // LIST has `cdl_name ASC` as the calendar-disambiguating
            // secondary (always ASC), plus the usual `UPPER(title)` tie-
            // breaker.
            SORT_LIST => format!("{expr} {dir}, cdl_name ASC, UPPER(title) ASC"),
            // Every other sort gets `UPPER(title) ASC` as the tie-breaker.
            _ => format!("{expr} {dir}, UPPER(title) ASC"),
        };
        format!("{sql} ORDER BY {with_secondary}")
    } else {
        sql
    };
    adjust_query_for_flags(prefs, sql)
}
