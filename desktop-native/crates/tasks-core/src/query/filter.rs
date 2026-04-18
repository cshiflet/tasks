//! The filter a task-list query is scoped to.
//!
//! Upstream (`org.tasks.filters.Filter`) this is an interface with several
//! subclasses (`CaldavFilter`, `RecentlyModifiedFilter`, `AstridOrderingFilter`,
//! custom saved filters, etc.). The native client only needs to know what
//! parent predicate to embed in the recursive CTE, so we collapse the
//! subtypes into a small enum.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryFilter {
    /// A built-in or user-defined filter whose SQL fragment (a `WHERE ...`
    /// clause optionally preceded by JOINs) is substituted verbatim. This
    /// is what `filters.sql` stores in the Android DB.
    Custom {
        sql: String,
        /// Some filters allow Astrid ordering; that path uses the
        /// non-recursive builder. Mirrors `AstridOrderingFilter`.
        supports_astrid_ordering: bool,
        /// `RecentlyModifiedFilter` is special-cased in the non-recursive
        /// builder to skip the "completed at bottom" sort prelude.
        is_recently_modified: bool,
    },
    /// A CalDAV / Google Tasks / Microsoft To Do list scoped by calendar
    /// UUID. Mirrors `CaldavFilter`.
    Caldav {
        calendar_uuid: String,
        is_google_tasks: bool,
    },
}

impl QueryFilter {
    pub fn custom(sql: impl Into<String>) -> Self {
        QueryFilter::Custom {
            sql: sql.into(),
            supports_astrid_ordering: false,
            is_recently_modified: false,
        }
    }

    pub fn recently_modified(sql: impl Into<String>) -> Self {
        QueryFilter::Custom {
            sql: sql.into(),
            supports_astrid_ordering: false,
            is_recently_modified: true,
        }
    }

    pub fn caldav(calendar_uuid: impl Into<String>) -> Self {
        QueryFilter::Caldav {
            calendar_uuid: calendar_uuid.into(),
            is_google_tasks: false,
        }
    }

    pub fn supports_manual_sort(&self) -> bool {
        matches!(self, QueryFilter::Caldav { .. })
    }
}
