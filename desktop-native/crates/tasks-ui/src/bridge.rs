//! cxx-qt bridge: `TaskListViewModel`.
//!
//! Exposes the read-only task list, the currently-selected task's detail
//! fields, and a sidebar of filters/CalDAV calendars to QML. The view
//! model opens the SQLite file the Android client writes (schema-hash
//! pinned), composes the recursive query via `tasks-core::query`, and
//! materialises the rows into parallel Q_PROPERTYs the QML layer indexes
//! by row position.
//!
//! Design note (see desktop-native/DECISIONS.md): we deliberately avoid
//! the QVariantMap/QVariantList round trip because cxx-qt-lib 0.7 does
//! not ship a `QVariant::from(&QVariantMap)` conversion. Parallel
//! QStringList / QList<i64> properties cover every field a ListView
//! delegate needs and compile cleanly against the stock cxx-qt types.

#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;

        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;

        include!("cxx-qt-lib/qlist.h");
        #[cxx_name = "QList_i64"]
        type QList_i64 = cxx_qt_lib::QList<i64>;
        #[cxx_name = "QList_i32"]
        type QList_i32 = cxx_qt_lib::QList<i32>;
        #[cxx_name = "QList_bool"]
        type QList_bool = cxx_qt_lib::QList<bool>;
    }

    // `auto_cxx_name` tells cxx-qt to snake_case → camelCase every
    // C++-side name it generates from this block (Q_PROPERTY names,
    // setters, change signals, Q_INVOKABLE methods). Without it,
    // cxx-qt 0.7 emits the raw Rust identifier, so QML bindings
    // written as `viewModel.dbPathDisplay` / `viewModel.sidebarLabels`
    // would silently resolve to `undefined` and the UI would render
    // with no data even though the bridge was otherwise healthy.
    #[auto_cxx_name]
    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        // List-pane data (parallel arrays indexed by row).
        #[qproperty(i32, count)]
        #[qproperty(QStringList, titles)]
        #[qproperty(QList_i64, task_ids)]
        #[qproperty(QList_i32, indents)]
        #[qproperty(QList_bool, completed_flags)]
        // Whether each row is recurring (`tasks.recurrence` non-empty).
        // The list pane swaps the priority-coloured checkbox for a
        // round arrows-loop indicator when this flag is true so the
        // user can spot recurring tasks at a glance, matching the
        // Android client's row design.
        #[qproperty(QList_bool, recurring_flags)]
        #[qproperty(QStringList, due_labels)]
        #[qproperty(QList_i32, priorities)]
        // H-7: per-row metadata so list rows can render at parity
        // with the Android client. `task_tag_summaries[i]` is the
        // comma-joined display names of tags attached to row `i`,
        // empty when the task has none. `task_list_names[i]` and
        // `task_list_colors[i]` are the display name + i32 ARGB
        // colour of the row's CalDAV list, both empty / 0 when
        // the task is local-only.
        #[qproperty(QStringList, task_tag_summaries)]
        // Per-task comma-joined tag UIDs, parallel to `task_ids`.
        // The list view splits on `,` and looks each UID up in the
        // global `tag_uids` / `tag_labels` / `tag_colors` arrays to
        // render coloured chips under each row's title. Empty when
        // the task has no tags.
        #[qproperty(QStringList, task_tag_uid_lists)]
        #[qproperty(QStringList, task_list_names)]
        #[qproperty(QList_i32, task_list_colors)]
        // Detail-pane data for the currently-selected task.
        #[qproperty(i64, selected_id)]
        #[qproperty(QString, selected_title)]
        #[qproperty(QString, selected_notes)]
        #[qproperty(QString, selected_due_label)]
        // Hide-until as the same round-trippable "YYYY-MM-DD [HH:MM]"
        // string the due field uses. Seeds the edit dialog; empty
        // when `tasks.hideUntil == 0`.
        #[qproperty(QString, selected_hide_until_label)]
        #[qproperty(i32, selected_priority)]
        #[qproperty(bool, selected_completed)]
        // Humanised RRULE ("Every week on Mon, Wed, Fri"). The raw
        // form lives in `tasks.recurrence`; this property is the
        // output of `tasks_core::recurrence::humanize_rrule` with
        // the from-completion suffix pre-applied, so the detail
        // pane can render it verbatim.
        #[qproperty(QString, selected_recurrence)]
        // CalDAV calendars available in the database, for the edit
        // dialog's "list" picker. Built from the same query the
        // sidebar uses; both lists stay in sync because they're
        // populated in `open_at_path`.
        #[qproperty(QStringList, caldav_calendar_labels)]
        #[qproperty(QStringList, caldav_calendar_uuids)]
        // UUID of the selected task's current CalDAV list, or empty
        // when the task has no caldav_tasks row (local-only).
        #[qproperty(QString, selected_caldav_calendar_uuid)]
        // i32 ARGB colour of the selected task's CalDAV list, or 0
        // when the task is local-only. Drives the coloured chip in
        // the detail pane.
        #[qproperty(i32, selected_caldav_calendar_color)]
        // All tag definitions from `tagdata`, parallel arrays for
        // the edit dialog's multi-select picker. `tag_uids` drives
        // the semantic (writes back via the update invokable);
        // `tag_labels` is the human-readable name.
        #[qproperty(QStringList, tag_labels)]
        #[qproperty(QStringList, tag_uids)]
        // i32 ARGB colour per tag, parallel to `tag_uids`. Drives
        // the coloured chip backgrounds in the detail pane's tag
        // strip; 0 means "no colour", which the QML side falls back
        // to a neutral grey for.
        #[qproperty(QList_i32, tag_colors)]
        // Tag UIDs currently attached to the selected task. Used
        // to pre-check the dialog's checkboxes on open.
        #[qproperty(QStringList, selected_tag_uids)]
        // Alarms attached to the selected task. Parallel arrays —
        // labels is the humanised description for display, times
        // and types are the raw columns the edit dialog passes
        // back into `updateSelectedTask`.
        #[qproperty(QStringList, selected_alarm_labels)]
        #[qproperty(QList_i64, selected_alarm_times)]
        #[qproperty(QList_i32, selected_alarm_types)]
        // Places known to the DB, parallel arrays for the edit
        // dialog's location picker.
        #[qproperty(QStringList, place_labels)]
        #[qproperty(QStringList, place_uids)]
        // Geofence currently attached to the selected task; empty
        // `selected_place_uid` means no geofence.
        #[qproperty(QString, selected_place_uid)]
        #[qproperty(bool, selected_place_arrival)]
        #[qproperty(bool, selected_place_departure)]
        // Candidate parents for the subtask picker. Labels are
        // task titles, IDs are tasks._id. Populated on open and
        // refreshed on every select_task so newly-added tasks
        // show up in the dropdown.
        #[qproperty(QStringList, parent_candidate_labels)]
        #[qproperty(QList_i64, parent_candidate_ids)]
        // Current parent task id for the selected row (0 = top-level).
        #[qproperty(i64, selected_parent_id)]
        // Timer fields rendered as H:MM strings for the edit
        // dialog. Empty string means zero.
        #[qproperty(QString, selected_estimated_text)]
        #[qproperty(QString, selected_elapsed_text)]
        // Raw `tasks.recurrence` (RRULE) + `tasks.repeat_from` for
        // the inline recurrence editor. Humanised summary is in
        // `selected_recurrence` for the detail pane's display;
        // these are the edit-dialog round-trip values.
        #[qproperty(QString, selected_recurrence_raw)]
        #[qproperty(i32, selected_repeat_from)]
        // Live values of the query preferences. Seeded from the
        // view model's `preferences` field; the preferences
        // dialog reads these to pre-fill its controls and calls
        // `updatePreferences` on save.
        #[qproperty(i32, pref_sort_mode)]
        #[qproperty(bool, pref_sort_ascending)]
        #[qproperty(bool, pref_show_completed)]
        #[qproperty(bool, pref_show_hidden)]
        #[qproperty(bool, pref_completed_at_bottom)]
        // Material theme override — persisted across restarts.
        //   0 = Follow OS, 1 = Light, 2 = Dark.
        // Surfaced in Settings → General → Appearance and read by
        // Main.qml's `appearanceTheme` binding.
        #[qproperty(i32, theme_mode)]
        // Sidebar: parallel label / identifier arrays. Identifier format:
        //   "__all__" | "__today__" | "__recent__"  (built-in filters)
        //   "caldav:<uuid>"                          (CalDAV calendar)
        //   "filter:<id>"                            (custom saved filter)
        #[qproperty(QStringList, sidebar_labels)]
        #[qproperty(QStringList, sidebar_ids)]
        // Parallel to `sidebar_ids`. -1 for built-in filters and
        // saved filters; for `caldav:<uuid>` rows it carries the
        // owning `caldav_accounts.cda_account_type` so the QML can
        // group LOCAL (2) lists separately from real CalDAV (0)
        // lists, and similarly distinguish Google Tasks / Microsoft
        // To Do / Etebase / Tasks.org / OpenTasks accounts. The id
        // prefix stays uniform because every row lives in
        // `caldav_lists`; the kind is purely a display concern.
        #[qproperty(QList_i32, sidebar_account_kinds)]
        // Parallel to `sidebar_ids` — the group key the QML uses
        // for collapsible-section logic. "filters_builtin" for the
        // top three built-ins, "saved" for `filter:*` rows, and
        // `account:<cda_uuid>` for an account header AND every
        // `caldav:` list row that hangs off it. Letting the bridge
        // emit the group key directly is simpler than re-deriving
        // it in QML by walking back over earlier indexes.
        #[qproperty(QStringList, sidebar_groups)]
        // i32 ARGB per row, parallel to `sidebar_ids`. Drives the
        // little colour dot the sidebar paints next to each list
        // label. 0 for non-list rows (built-ins / account headers /
        // saved filters); for `caldav:` rows it's the
        // `caldav_lists.cdl_color` value.
        #[qproperty(QList_i32, sidebar_colors)]
        #[qproperty(QString, active_filter_id)]
        // Configured sync accounts, parallel arrays for the Settings
        // → Accounts pane. `account_kinds` is the integer tag
        //   0 = CalDAV, 1 = Google Tasks, 2 = Microsoft To Do, 3 = EteSync
        // matching `tasks_sync::ProviderKind`. Server + username are
        // blank for OAuth providers (they come from the eventual
        // token store); passwords are held in-memory only on the Rust
        // side and never exposed as a Q_PROPERTY.
        #[qproperty(QStringList, account_labels)]
        #[qproperty(QList_i32, account_kinds)]
        #[qproperty(QStringList, account_servers)]
        #[qproperty(QStringList, account_usernames)]
        // `caldav_accounts.cda_uuid` per row, parallel to the
        // other account_* arrays. QML hands a uuid to
        // `sync_account` to point the engine at the right row.
        #[qproperty(QStringList, account_uuids)]
        // Per-account user-facing sync state ("idle", "syncing…",
        // "Synced 5s ago", "Sync failed: …"). Updated synchronously
        // around each `sync_account` invokable; the QML row binds
        // its trailing label to the matching index.
        #[qproperty(QStringList, account_sync_states)]
        // H-6: id of the last task soft-deleted in this session,
        // valid until the toast countdown expires or the user
        // restores it. 0 = nothing to undo. The toast Popup
        // surfaces an Undo button while this is non-zero. The
        // deleted task's title is held only on the Rust side
        // (used to format the status-line message); QML doesn't
        // need it directly.
        #[qproperty(i64, last_deleted_id)]
        // Status bar text.
        #[qproperty(QString, status)]
        // Inline result string from the most recent
        // `test_account_connection` invokable. Updated to
        // "Testing connection…" / "Test successful — credentials
        // work." / "Test failed: …" so the Accounts pane can
        // render the verdict next to the Test button instead of
        // routing it through the status bar at the bottom.
        #[qproperty(QString, last_test_result)]
        // Absolute path of the currently-open database, surfaced in
        // the window title + Browse path field so users know which
        // file they're looking at.
        #[qproperty(QString, db_path_display)]
        type TaskListViewModel = super::TaskListViewModelRust;

        #[qinvokable]
        fn open_database(self: Pin<&mut TaskListViewModel>, path: QString);

        #[qinvokable]
        fn open_default_database(self: Pin<&mut TaskListViewModel>);

        #[qinvokable]
        fn import_json_backup(self: Pin<&mut TaskListViewModel>, path: QString);

        #[qinvokable]
        fn select_filter(self: Pin<&mut TaskListViewModel>, id: QString);

        #[qinvokable]
        fn select_task(self: Pin<&mut TaskListViewModel>, id: i64);

        /// Create a brand-new task with the given title. If the
        /// active filter is a CalDAV list, the new task is
        /// assigned to that list automatically.
        #[qinvokable]
        fn add_new_task(self: Pin<&mut TaskListViewModel>, title: QString);

        /// Apply new query preferences and reload the active
        /// filter. Session-local only for now; QSettings
        /// persistence is a follow-up.
        #[qinvokable]
        fn update_preferences(
            self: Pin<&mut TaskListViewModel>,
            sort_mode: i32,
            sort_ascending: bool,
            show_completed: bool,
            show_hidden: bool,
            completed_at_bottom: bool,
        );

        /// Update + persist the appearance theme (0=System, 1=Light, 2=Dark).
        /// QML calls this from Settings → General → Appearance.
        #[qinvokable]
        fn update_theme_mode(self: Pin<&mut TaskListViewModel>, mode: i32);

        #[qinvokable]
        fn toggle_task_completion(self: Pin<&mut TaskListViewModel>, id: i64, completed: bool);

        #[qinvokable]
        fn delete_selected_task(self: Pin<&mut TaskListViewModel>);

        /// Apply the edit-dialog's form state to the currently-
        /// selected task. `due_text` / `hide_until_text` are parsed
        /// via `tasks_core::datetime::parse_due_input`; an empty
        /// string means "no date". `caldav_uuid` reassigns the
        /// task's CalDAV calendar when non-empty (no-op for local
        /// tasks that don't have a caldav_tasks row). A parse
        /// failure surfaces on the status line and leaves the DB
        /// untouched.
        #[qinvokable]
        fn update_selected_task(
            self: Pin<&mut TaskListViewModel>,
            title: QString,
            notes: QString,
            due_text: QString,
            hide_until_text: QString,
            priority: i32,
            caldav_uuid: QString,
            tag_uids_list: QStringList,
            alarm_times: QList_i64,
            alarm_types: QList_i32,
            place_uid: QString,
            place_arrival: bool,
            place_departure: bool,
            parent_id: i64,
            estimate_text: QString,
            elapsed_text: QString,
            recurrence: QString,
            repeat_from: i32,
        );

        /// Persist a password-auth sync account (CalDAV or EteSync)
        /// to the in-memory accounts list and re-emit the Q_PROPERTY
        /// arrays the Accounts pane binds to. `kind` must be 0
        /// (CalDAV) or 3 (EteSync); other values are rejected on the
        /// status line. Empty required fields are also rejected.
        ///
        /// Session-local only for now — neither the account list nor
        /// the password survives a restart. OS-native keychain
        /// storage (libsecret / Keychain / Credential Manager) is
        /// the follow-up tracked in PLAN_UPDATES §11.
        #[qinvokable]
        fn add_password_account(
            self: Pin<&mut TaskListViewModel>,
            kind: i32,
            label: QString,
            server: QString,
            username: QString,
            password: QString,
        );

        /// Drop the account at `index`. Out-of-range indices are
        /// ignored (the QML row shouldn't be able to produce one,
        /// but paranoia is cheap).
        #[qinvokable]
        fn remove_account(self: Pin<&mut TaskListViewModel>, index: i32);

        /// Mutate the account row identified by `cda_uuid` in place.
        /// Used by the Accounts pane's Edit dialog. Empty
        /// `password` keeps the existing one (so the user doesn't
        /// have to retype to change the label / username / URL).
        #[qinvokable]
        fn update_password_account(
            self: Pin<&mut TaskListViewModel>,
            cda_uuid: QString,
            label: QString,
            server: QString,
            username: QString,
            password: QString,
        );

        /// Try connecting with the supplied credentials without
        /// persisting anything. Surfaces the result on the status
        /// bar so the Accounts pane's Test button can verify a
        /// server-URL / username / password combination before the
        /// user clicks Add account. `kind` matches the same
        /// integers `add_password_account` uses.
        #[qinvokable]
        fn test_account_connection(
            self: Pin<&mut TaskListViewModel>,
            kind: i32,
            server: QString,
            username: QString,
            password: QString,
        );

        /// Drive a one-shot pull-then-push cycle against the account
        /// identified by `cda_uuid`. Returns immediately — the
        /// actual cycle runs on a background worker thread, and a
        /// `qt_thread.queue` callback rejoins the QML thread to
        /// publish the result.
        #[qinvokable]
        fn sync_account(self: Pin<&mut TaskListViewModel>, cda_uuid: QString);

        /// Dispatch a sync against every non-local account. Wired
        /// to the toolbar's manual Sync button. Same async dispatch
        /// model as `sync_account`; status flows through the
        /// status-bar text per account.
        #[qinvokable]
        fn sync_all_accounts(self: Pin<&mut TaskListViewModel>);

        /// Create a calendar / task list on `cda_uuid`'s server with
        /// the given display name and (optional) i32 ARGB colour.
        /// On success, runs a sync against that account so the new
        /// list lands in `caldav_lists` and the sidebar repopulates.
        #[qinvokable]
        fn create_account_calendar(
            self: Pin<&mut TaskListViewModel>,
            cda_uuid: QString,
            name: QString,
            color: i32,
        );

        /// Update the visual appearance of an existing list (colour
        /// for now; icon will land alongside the icon-font work).
        /// Writes `caldav_lists.cdl_color` directly and refreshes
        /// the sidebar / active filter so the change paints
        /// immediately in the chip strip + per-row checkbox tint.
        #[qinvokable]
        fn update_list_color(self: Pin<&mut TaskListViewModel>, cdl_uuid: QString, color: i32);

        /// H-4: free-text substring search across task title +
        /// notes. Empty input restores the currently-active filter.
        /// Called from the toolbar search field on every text edit.
        #[qinvokable]
        fn set_search_query(self: Pin<&mut TaskListViewModel>, query: QString);

        /// H-6: undo the most recent delete-from-detail-pane.
        /// No-op when `last_deleted_id` is 0 (already restored or
        /// never deleted). The undo toast button calls this on
        /// click; the toast also calls `clearLastDeleted` when the
        /// hide-timer fires so a stale id doesn't keep the button
        /// active forever.
        #[qinvokable]
        fn restore_last_deleted(self: Pin<&mut TaskListViewModel>);

        /// Reset the `last_deleted_id` Q_PROPERTY to 0 — called
        /// from the toast when its hide-timer expires so the Undo
        /// button disappears at the same time the toast does.
        #[qinvokable]
        fn clear_last_deleted(self: Pin<&mut TaskListViewModel>);
    }

    // Opt the view model into cxx-qt's Threading surface so the
    // filesystem-watcher thread can queue reloads back on the Qt
    // event loop thread.
    impl cxx_qt::Threading for TaskListViewModel {}
}

use core::pin::Pin;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::{QDateTime, QList, QString, QStringList};
use secrecy::SecretString;

use tasks_core::datetime::{
    describe_alarm, format_due_label, format_duration_hhmm, parse_due_input, parse_duration_input,
};
use tasks_core::db::{default_db_path, Database};
use tasks_core::models::{CaldavCalendar, Filter as CustomFilter, Priority, RepeatFrom, Task};
use tasks_core::query::{
    run_by_filter_id, run_search, QueryPreferences, FILTER_ALL, FILTER_RECENT, FILTER_TODAY,
};
use tasks_core::recurrence::humanize_rrule;
use tasks_core::watch::DatabaseWatcher;
use tasks_sync::providers::{caldav::CalDavProvider, etesync::EteSyncProvider};
use tasks_sync::{AccountCredentials, Provider, SyncEngine};

/// Provider kind tags that match `tasks_sync::ProviderKind` in
/// numeric order. Kept as bare integers at the bridge boundary so
/// QML can pass the picker's index through without a named type.
const KIND_CALDAV: i32 = 0;
const KIND_GOOGLE_TASKS: i32 = 1;
const KIND_MICROSOFT_TODO: i32 = 2;
const KIND_ETESYNC: i32 = 3;

/// Non-Qt account record. `password` is held on the Rust side only
/// so it never crosses the FFI boundary into QML. Cleared when the
/// view model is dropped; OS-native keychain storage lands with the
/// follow-up tracked in PLAN_UPDATES §11.
///
/// `password` is captured but not yet consumed — the SyncEngine
/// wiring is the next commit. Silencing dead_code until then so the
/// type signature stays stable across the two commits.
///
/// `password` is wrapped in [`SecretString`] (M-1) so it zeroes on
/// drop and can't be accidentally Debug-printed alongside the rest
/// of the struct. Its single consumer — the future SyncEngine
/// handoff — exposes the inner value only at the FFI boundary via
/// `.expose_secret()`.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct StoredAccount {
    /// `caldav_accounts.cda_uuid` of the row this StoredAccount
    /// represents. Generated when the account is added via the
    /// Accounts pane (uuid::Uuid::new_v4); QML passes it back into
    /// `sync_account` to identify the row to refresh.
    uuid: String,
    kind: i32,
    label: String,
    server: String,
    username: String,
    password: SecretString,
}

pub struct TaskListViewModelRust {
    // List state.
    count: i32,
    titles: QStringList,
    task_ids: QList<i64>,
    indents: QList<i32>,
    completed_flags: QList<bool>,
    recurring_flags: QList<bool>,
    due_labels: QStringList,
    priorities: QList<i32>,
    task_tag_summaries: QStringList,
    task_tag_uid_lists: QStringList,
    task_list_names: QStringList,
    task_list_colors: QList<i32>,
    // Detail state.
    selected_id: i64,
    selected_title: QString,
    selected_notes: QString,
    selected_due_label: QString,
    selected_hide_until_label: QString,
    selected_priority: i32,
    selected_completed: bool,
    selected_recurrence: QString,
    caldav_calendar_labels: QStringList,
    caldav_calendar_uuids: QStringList,
    selected_caldav_calendar_uuid: QString,
    selected_caldav_calendar_color: i32,
    tag_labels: QStringList,
    tag_uids: QStringList,
    tag_colors: QList<i32>,
    selected_tag_uids: QStringList,
    selected_alarm_labels: QStringList,
    selected_alarm_times: QList<i64>,
    selected_alarm_types: QList<i32>,
    place_labels: QStringList,
    place_uids: QStringList,
    selected_place_uid: QString,
    selected_place_arrival: bool,
    selected_place_departure: bool,
    parent_candidate_labels: QStringList,
    parent_candidate_ids: QList<i64>,
    selected_parent_id: i64,
    selected_estimated_text: QString,
    selected_elapsed_text: QString,
    selected_recurrence_raw: QString,
    selected_repeat_from: i32,
    pref_sort_mode: i32,
    pref_sort_ascending: bool,
    pref_show_completed: bool,
    pref_show_hidden: bool,
    pref_completed_at_bottom: bool,
    theme_mode: i32,
    // Sidebar state.
    sidebar_labels: QStringList,
    sidebar_ids: QStringList,
    sidebar_account_kinds: QList<i32>,
    sidebar_groups: QStringList,
    sidebar_colors: QList<i32>,
    active_filter_id: QString,
    // H-4: free-text substring search across title + notes. When
    // non-empty, `reload_active_filter` runs `run_search` instead
    // of `run_by_filter_id`. Held on the Rust side only; the QML
    // search field reads/writes its own `text` and pushes via the
    // `setSearchQuery` invokable.
    search_query: String,
    // Accounts state (parallel arrays; see Q_PROPERTY comments above).
    account_labels: QStringList,
    account_kinds: QList<i32>,
    account_servers: QStringList,
    account_usernames: QStringList,
    account_uuids: QStringList,
    account_sync_states: QStringList,
    // Non-Qt account storage: keeps the password alongside the
    // user-facing fields without exposing it to QML. Session-local.
    accounts: Vec<StoredAccount>,
    /// Parallel to `accounts` — current sync state string for each
    /// row, mirrored into the `account_sync_states` Q_PROPERTY.
    /// Lives on the Rust side so we can patch a single index
    /// without round-tripping through QStringList (cxx-qt 0.7
    /// doesn't expose an index setter on QStringList).
    account_states: Vec<String>,
    // H-6: last-deleted-task pinning for the undo flow. The id
    // crosses FFI as a Q_PROPERTY (so QML can show / hide the
    // Undo button); the title stays Rust-side because nothing in
    // QML needs the raw string.
    last_deleted_id: i64,
    last_deleted_title: String,
    // Status.
    status: QString,
    last_test_result: QString,
    db_path_display: QString,
    // Non-Qt bookkeeping. Held on the Rust side only; not exposed to QML.
    db_path: Option<PathBuf>,
    db: Option<Database>,
    task_cache: Vec<Task>,
    /// User preferences fed to `run_by_filter_id`. The UI panel for
    /// editing these isn't wired yet (Milestone 1 scope); for now it
    /// stays at the Android defaults.
    preferences: QueryPreferences,
    /// Flag the filesystem-watcher thread checks periodically to exit
    /// when a new DB is opened (or the view model is dropped). Shared
    /// with the spawned thread via `Arc`. `None` when no watcher is
    /// currently active.
    watcher_stop: Option<Arc<AtomicBool>>,
    /// Multi-threaded tokio runtime hosting `tasks-sync` calls.
    /// Built lazily on first sync_account invocation; one Runtime
    /// instance is shared across every Sync now click for the
    /// lifetime of the view model.
    runtime: Option<tokio::runtime::Runtime>,
}

impl Default for TaskListViewModelRust {
    fn default() -> Self {
        // Load on construction so the view model's Q_PROPERTYs come
        // up reflecting whatever the user picked last session. A
        // missing / malformed file falls back to the struct's
        // own defaults.
        let saved = crate::preferences::Preferences::load();
        TaskListViewModelRust {
            count: 0,
            titles: QStringList::default(),
            task_ids: QList::default(),
            indents: QList::default(),
            completed_flags: QList::default(),
            recurring_flags: QList::default(),
            due_labels: QStringList::default(),
            priorities: QList::default(),
            task_tag_summaries: QStringList::default(),
            task_tag_uid_lists: QStringList::default(),
            task_list_names: QStringList::default(),
            task_list_colors: QList::default(),
            selected_id: 0,
            selected_title: QString::default(),
            selected_notes: QString::default(),
            selected_due_label: QString::default(),
            selected_hide_until_label: QString::default(),
            selected_priority: Priority::NONE,
            selected_completed: false,
            selected_recurrence: QString::default(),
            caldav_calendar_labels: QStringList::default(),
            caldav_calendar_uuids: QStringList::default(),
            selected_caldav_calendar_uuid: QString::default(),
            selected_caldav_calendar_color: 0,
            tag_labels: QStringList::default(),
            tag_uids: QStringList::default(),
            tag_colors: QList::default(),
            selected_tag_uids: QStringList::default(),
            selected_alarm_labels: QStringList::default(),
            selected_alarm_times: QList::default(),
            selected_alarm_types: QList::default(),
            place_labels: QStringList::default(),
            place_uids: QStringList::default(),
            selected_place_uid: QString::default(),
            selected_place_arrival: false,
            selected_place_departure: false,
            parent_candidate_labels: QStringList::default(),
            parent_candidate_ids: QList::default(),
            selected_parent_id: 0,
            selected_estimated_text: QString::default(),
            selected_elapsed_text: QString::default(),
            selected_recurrence_raw: QString::default(),
            selected_repeat_from: 0,
            // Seed from the persisted Preferences blob — falls back
            // to Android defaults when nothing is saved. The
            // QueryPreferences struct further down carries the same
            // values so the Q_PROPERTYs and the in-memory query
            // input stay in sync from the first paint.
            pref_sort_mode: saved.sort_mode,
            pref_sort_ascending: saved.sort_ascending,
            pref_show_completed: saved.show_completed,
            pref_show_hidden: saved.show_hidden,
            pref_completed_at_bottom: saved.completed_at_bottom,
            theme_mode: saved.theme_mode,
            sidebar_labels: QStringList::default(),
            sidebar_ids: QStringList::default(),
            sidebar_account_kinds: QList::default(),
            sidebar_groups: QStringList::default(),
            sidebar_colors: QList::default(),
            active_filter_id: QString::from(FILTER_ALL),
            search_query: String::new(),
            account_labels: QStringList::default(),
            account_kinds: QList::default(),
            account_servers: QStringList::default(),
            account_usernames: QStringList::default(),
            account_uuids: QStringList::default(),
            account_sync_states: QStringList::default(),
            accounts: Vec::new(),
            account_states: Vec::new(),
            last_deleted_id: 0,
            last_deleted_title: String::new(),
            status: QString::default(),
            last_test_result: QString::default(),
            db_path_display: QString::default(),
            db_path: None,
            db: None,
            task_cache: Vec::new(),
            preferences: QueryPreferences {
                sort_mode: saved.sort_mode,
                sort_ascending: saved.sort_ascending,
                show_completed: saved.show_completed,
                show_hidden: saved.show_hidden,
                completed_tasks_at_bottom: saved.completed_at_bottom,
                ..QueryPreferences::default()
            },
            watcher_stop: None,
            runtime: None,
        }
    }
}

impl Drop for TaskListViewModelRust {
    fn drop(&mut self) {
        // Ensure the watcher thread exits when the view model does.
        if let Some(stop) = self.watcher_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }
}

impl qobject::TaskListViewModel {
    /// Open a user-specified database file read-only. Used by the
    /// Browse… button in the QML toolbar.
    pub fn open_database(self: Pin<&mut Self>, path: QString) {
        let path_buf = PathBuf::from(path.to_string());
        open_at_path(self, path_buf, OpenMode::ReadOnlyOnly);
    }

    /// Open the desktop client's managed database at the default
    /// per-OS path, creating an empty schema if the file doesn't
    /// exist yet. Called from `Main.qml`'s `Component.onCompleted`
    /// so users don't have to pick a file on first launch.
    pub fn open_default_database(self: Pin<&mut Self>) {
        match default_db_path() {
            Some(path) => open_at_path(self, path, OpenMode::CreateIfMissing),
            None => {
                // No resolvable data directory — very rare; surface
                // to the status bar and let the user pick via Browse.
                self.set_status(QString::from(
                    "Couldn't resolve a default data directory; use Browse\u{2026}",
                ));
            }
        }
    }

    /// Import a Tasks.org JSON backup (the file produced by the
    /// Android app's Settings → Backups → Export JSON flow) into
    /// the currently-open database. Tears down the watcher while
    /// the write happens, then reopens the DB read-only and reloads
    /// the active filter so the new rows show up immediately.
    pub fn import_json_backup(mut self: Pin<&mut Self>, path: QString) {
        let source = PathBuf::from(path.to_string());
        // Import targets the currently-open DB. If none is open yet
        // (first launch, pre-openDefault), surface a clear error
        // rather than silently targeting the default.
        let target = match self.db_path.clone() {
            Some(p) => p,
            None => {
                self.as_mut().set_status(QString::from(
                    "Open or create a database first, then import.",
                ));
                return;
            }
        };

        // Close the read-only handle + stop the watcher so the
        // importer's writable open has exclusive access. The target
        // is our own file; no other process touches it.
        stop_prior_watcher(self.as_mut());
        self.as_mut().rust_mut().db = None;

        let outcome = tasks_core::import::import_json_backup(&target, &source);
        match outcome {
            Ok(stats) => {
                let msg = format!(
                    "Imported {} tasks, {} places, {} tags, {} filters from {}",
                    stats.tasks,
                    stats.places,
                    stats.tag_data,
                    stats.filters,
                    source.display()
                );
                tracing::info!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
                // Reopen the DB read-only and refresh the views.
                open_at_path(self.as_mut(), target, OpenMode::ReadOnlyOnly);
            }
            Err(e) => {
                let msg = format!("Import failed: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
                // Reopen the target so the UI isn't stuck in a closed
                // state.
                open_at_path(self.as_mut(), target, OpenMode::ReadOnlyOnly);
            }
        }
    }

    pub fn select_filter(mut self: Pin<&mut Self>, id: QString) {
        self.as_mut().set_active_filter_id(id);
        self.as_mut().reload_active_filter();
    }

    /// Apply + persist new list-default query preferences and reload.
    pub fn update_preferences(
        mut self: Pin<&mut Self>,
        sort_mode: i32,
        sort_ascending: bool,
        show_completed: bool,
        show_hidden: bool,
        completed_at_bottom: bool,
    ) {
        {
            let mut inner = self.as_mut().rust_mut();
            inner.preferences.sort_mode = sort_mode;
            inner.preferences.sort_ascending = sort_ascending;
            inner.preferences.show_completed = show_completed;
            inner.preferences.show_hidden = show_hidden;
            inner.preferences.completed_tasks_at_bottom = completed_at_bottom;
        }
        self.as_mut().set_pref_sort_mode(sort_mode);
        self.as_mut().set_pref_sort_ascending(sort_ascending);
        self.as_mut().set_pref_show_completed(show_completed);
        self.as_mut().set_pref_show_hidden(show_hidden);
        self.as_mut()
            .set_pref_completed_at_bottom(completed_at_bottom);
        persist_prefs(self.as_ref().get_ref());
        self.as_mut().reload_active_filter();
    }

    /// Apply + persist the appearance theme override.
    pub fn update_theme_mode(mut self: Pin<&mut Self>, mode: i32) {
        let clamped = mode.clamp(0, 2);
        self.as_mut().set_theme_mode(clamped);
        persist_prefs(self.as_ref().get_ref());
    }

    /// Add a CalDAV or EteSync account to the session's accounts
    /// list. Validates kind + required fields up front so the
    /// Accounts pane gets a single-line status message on reject
    /// instead of a silent drop.
    pub fn add_password_account(
        mut self: Pin<&mut Self>,
        kind: i32,
        label: QString,
        server: QString,
        username: QString,
        password: QString,
    ) {
        if kind != KIND_CALDAV && kind != KIND_ETESYNC {
            let msg = match kind {
                KIND_GOOGLE_TASKS => {
                    "Google Tasks sign-in lands with the OAuth flow (PLAN_UPDATES \u{00A7}11)."
                }
                KIND_MICROSOFT_TODO => {
                    "Microsoft To Do sign-in lands with the OAuth flow (PLAN_UPDATES \u{00A7}11)."
                }
                _ => "Unknown account type.",
            };
            self.as_mut().set_status(QString::from(msg));
            return;
        }
        let label_s = label.to_string().trim().to_string();
        let server_s = server.to_string().trim().to_string();
        let username_s = username.to_string().trim().to_string();
        let password_s = password.to_string();
        if label_s.is_empty()
            || server_s.is_empty()
            || username_s.is_empty()
            || password_s.is_empty()
        {
            self.as_mut().set_status(QString::from(
                "All four fields (label, server, username, password) are required.",
            ));
            return;
        }
        let uuid = uuid::Uuid::new_v4().to_string();

        // Persist into `caldav_accounts` so the bridge's sync path
        // (and future restarts) can find this row by uuid. This is
        // a session-local DB write; the password lands in plaintext
        // for now, mirroring how the JSON import stores it. OS-
        // keychain integration is tracked in PLAN_UPDATES §11.
        let cda_account_type = match kind {
            KIND_CALDAV => 0,  // tasks_core::AccountType::CALDAV
            KIND_ETESYNC => 5, // tasks_core::AccountType::ETEBASE
            _ => unreachable!("kind validated above"),
        };
        if let Some(path) = self.db_path.clone() {
            let res = open_rw_conn(&path).and_then(|conn| {
                conn.execute(
                    "INSERT OR REPLACE INTO caldav_accounts \
                     (cda_uuid, cda_name, cda_url, cda_username, cda_password, cda_error, \
                      cda_account_type, cda_collapsed, cda_server_type, cda_last_sync) \
                     VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, 0, -1, 0)",
                    rusqlite::params![
                        uuid,
                        label_s,
                        server_s,
                        username_s,
                        password_s,
                        cda_account_type,
                    ],
                )
                .map(|_| ())
            });
            if let Err(e) = res {
                self.as_mut()
                    .set_status(QString::from(&format!("DB write failed: {e}")));
                return;
            }
        } else {
            self.as_mut()
                .set_status(QString::from("Open a database before adding an account."));
            return;
        }

        {
            let mut inner = self.as_mut().rust_mut();
            inner.accounts.push(StoredAccount {
                uuid,
                kind,
                label: label_s,
                server: server_s,
                username: username_s,
                password: SecretString::from(password_s),
            });
            inner.account_states.push(String::from("Idle"));
        }
        publish_accounts(self.as_mut());
        self.as_mut()
            .set_status(QString::from("Account saved (session-local)."));
    }

    /// H-4: update the search query and reload. Empty string
    /// returns to the active filter; non-empty runs `run_search`
    /// against title + notes. Called on every QML toolbar text
    /// change so the list updates as the user types.
    pub fn set_search_query(mut self: Pin<&mut Self>, query: QString) {
        let s = query.to_string();
        let trimmed = s.trim();
        // Avoid unnecessary reloads if nothing meaningful changed.
        if trimmed == self.search_query {
            return;
        }
        self.as_mut().rust_mut().search_query = trimmed.to_string();
        self.as_mut().reload_active_filter();
    }

    /// Drop the account at `index`. Also removes the corresponding
    /// `caldav_accounts` row plus any `caldav_lists` / `caldav_tasks`
    /// hanging off it via the FK chain so the sidebar's lists for
    /// the deleted account disappear on the next reload.
    pub fn remove_account(mut self: Pin<&mut Self>, index: i32) {
        let idx = index as usize;
        if index < 0 || idx >= self.accounts.len() {
            return;
        }
        let removed = self.as_mut().rust_mut().accounts.remove(idx);
        if idx < self.account_states.len() {
            self.as_mut().rust_mut().account_states.remove(idx);
        }
        if let Some(path) = self.db_path.clone() {
            if let Ok(conn) = open_rw_conn(&path) {
                // Tear down child rows first so a missing FK CASCADE
                // doesn't leave orphans in the lists / tasks tables.
                let _ = conn.execute(
                    "DELETE FROM caldav_tasks WHERE cd_calendar IN \
                     (SELECT cdl_uuid FROM caldav_lists WHERE cdl_account = ?1)",
                    rusqlite::params![removed.uuid],
                );
                let _ = conn.execute(
                    "DELETE FROM caldav_lists WHERE cdl_account = ?1",
                    rusqlite::params![removed.uuid],
                );
                let _ = conn.execute(
                    "DELETE FROM caldav_accounts WHERE cda_uuid = ?1",
                    rusqlite::params![removed.uuid],
                );
            }
        }
        publish_accounts(self.as_mut());
        // Refresh sidebar so the removed account's lists disappear.
        if let Some(db) = &self.db {
            let (labels, ids, kinds, groups, colors) = build_sidebar(db);
            self.as_mut()
                .set_sidebar_labels(string_list_from_iter(labels.iter().map(String::as_str)));
            self.as_mut()
                .set_sidebar_ids(string_list_from_iter(ids.iter().map(String::as_str)));
            let mut kl: QList<i32> = QList::default();
            for k in &kinds {
                kl.append(*k);
            }
            self.as_mut().set_sidebar_account_kinds(kl);
            self.as_mut()
                .set_sidebar_groups(string_list_from_iter(groups.iter().map(String::as_str)));
            let mut cl: QList<i32> = QList::default();
            for c in &colors {
                cl.append(*c);
            }
            self.as_mut().set_sidebar_colors(cl);
        }
        self.as_mut().reload_active_filter();
        self.as_mut()
            .set_status(QString::from(&format!("Removed \"{}\".", removed.label)));
    }

    /// Update an existing account's editable fields (label, server,
    /// username, password). Empty `password` preserves the current
    /// value — the dialog leaves the password field blank by
    /// default so the user only re-enters it when changing it.
    pub fn update_password_account(
        mut self: Pin<&mut Self>,
        cda_uuid: QString,
        label: QString,
        server: QString,
        username: QString,
        password: QString,
    ) {
        let uuid = cda_uuid.to_string();
        let label_s = label.to_string().trim().to_string();
        let server_s = server.to_string().trim().to_string();
        let username_s = username.to_string().trim().to_string();
        let password_s = password.to_string();
        if label_s.is_empty() || server_s.is_empty() || username_s.is_empty() {
            self.as_mut()
                .set_status(QString::from("Label, server, and username are required."));
            return;
        }
        let Some(idx) = self.accounts.iter().position(|a| a.uuid == uuid) else {
            self.as_mut()
                .set_status(QString::from(&format!("No account with uuid {uuid}.")));
            return;
        };
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("Open a database before editing an account."));
            return;
        };
        // Persist via a transient RW connection. Empty password
        // preserves the existing one — caller blank-defaults the
        // field so a user fixing a typo doesn't have to retype the
        // password.
        let res = open_rw_conn(&path).and_then(|conn| {
            if password_s.is_empty() {
                conn.execute(
                    "UPDATE caldav_accounts \
                     SET cda_name = ?1, cda_url = ?2, cda_username = ?3 \
                     WHERE cda_uuid = ?4",
                    rusqlite::params![label_s, server_s, username_s, uuid],
                )
                .map(|_| ())
            } else {
                conn.execute(
                    "UPDATE caldav_accounts \
                     SET cda_name = ?1, cda_url = ?2, cda_username = ?3, cda_password = ?4 \
                     WHERE cda_uuid = ?5",
                    rusqlite::params![label_s, server_s, username_s, password_s, uuid],
                )
                .map(|_| ())
            }
        });
        if let Err(e) = res {
            self.as_mut()
                .set_status(QString::from(&format!("DB write failed: {e}")));
            return;
        }
        {
            let mut inner = self.as_mut().rust_mut();
            let acct = &mut inner.accounts[idx];
            acct.label = label_s.clone();
            acct.server = server_s;
            acct.username = username_s;
            if !password_s.is_empty() {
                acct.password = SecretString::from(password_s);
            }
        }
        publish_accounts(self.as_mut());
        self.as_mut()
            .set_status(QString::from(&format!("Updated \"{}\".", label_s)));
    }

    /// Try `provider.connect()` against `(kind, server, username,
    /// password)` without persisting anything. The Accounts pane's
    /// Test button uses this to verify credentials before the user
    /// commits via Add account. Status-bar text reports success or
    /// the underlying error message.
    pub fn test_account_connection(
        mut self: Pin<&mut Self>,
        kind: i32,
        server: QString,
        username: QString,
        password: QString,
    ) {
        if kind != KIND_CALDAV && kind != KIND_ETESYNC {
            self.as_mut()
                .set_status(QString::from("Test only supports CalDAV / EteSync today."));
            return;
        }
        let server_s = server.to_string().trim().to_string();
        let username_s = username.to_string().trim().to_string();
        let password_s = password.to_string();
        if server_s.is_empty() || username_s.is_empty() || password_s.is_empty() {
            self.as_mut().set_last_test_result(QString::from(
                "Server, username, and password are required to test.",
            ));
            return;
        }
        if self.runtime.is_none() {
            match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .thread_name("tasks-sync")
                .build()
            {
                Ok(rt) => self.as_mut().rust_mut().runtime = Some(rt),
                Err(e) => {
                    self.as_mut()
                        .set_status(QString::from(&format!("Couldn't start runtime: {e}")));
                    return;
                }
            }
        }
        self.as_mut()
            .set_last_test_result(QString::from("Testing connection…"));
        let creds = AccountCredentials::new_password(&server_s, &username_s, password_s);
        let mut provider: Box<dyn Provider + Send> = match kind {
            KIND_CALDAV => Box::new(CalDavProvider::new(creds, "test")),
            KIND_ETESYNC => Box::new(EteSyncProvider::new(creds, "test")),
            _ => unreachable!("kind validated above"),
        };
        let result = self
            .as_mut()
            .rust_mut()
            .runtime
            .as_ref()
            .expect("runtime constructed above")
            .block_on(async move { provider.connect().await });
        match result {
            Ok(()) => self
                .as_mut()
                .set_last_test_result(QString::from("Test successful — credentials work.")),
            Err(e) => self
                .as_mut()
                .set_last_test_result(QString::from(&format!("Test failed: {e}"))),
        }
    }

    /// Run a single sync cycle (pull + push) against the account
    /// keyed by `cda_uuid`. Synchronous wrt. the QML caller — the
    /// app freezes until the cycle finishes. Acceptable for a
    /// manual Sync now button against a local test server; the
    /// proper background-thread + signal version comes later.
    ///
    /// On success: rebuilds the sidebar so newly-pulled calendars
    /// appear, and reloads the active filter so any newly-pulled
    /// tasks show up in the list pane.
    /// Run a single sync cycle (pull + push) against the account
    /// keyed by `cda_uuid`. Returns immediately — the actual cycle
    /// runs on the background tokio runtime, and a `qt_thread.queue`
    /// callback rejoins the QML thread once it finishes to update
    /// the sidebar + status bar. Status surfaces as
    /// "Syncing <label>…" while in flight, then "<label>: Done" or
    /// "Sync of <label> failed: …" on completion.
    pub fn sync_account(mut self: Pin<&mut Self>, cda_uuid: QString) {
        let uuid = cda_uuid.to_string();
        let Some(stored) = self.accounts.iter().find(|a| a.uuid == uuid).cloned() else {
            self.as_mut()
                .set_status(QString::from(&format!("No account with uuid {uuid}")));
            return;
        };
        let Some(db_path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("Open a database before syncing."));
            return;
        };

        // Build (or reuse) the tokio runtime. Construction can fail
        // if the OS refuses thread spawn — surface that on the
        // status bar rather than panicking.
        if self.runtime.is_none() {
            match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .thread_name("tasks-sync")
                .build()
            {
                Ok(rt) => self.as_mut().rust_mut().runtime = Some(rt),
                Err(e) => {
                    self.as_mut()
                        .set_status(QString::from(&format!("Couldn't start sync runtime: {e}")));
                    return;
                }
            }
        }

        // Build the right Provider before we leave the QML thread
        // — the credential plumbing reads from `self.accounts` and
        // needs the `&self` borrow.
        let creds = AccountCredentials::new_password(
            &stored.server,
            &stored.username,
            secrecy::ExposeSecret::expose_secret(&stored.password).to_string(),
        );
        let label = stored.label.clone();
        let provider: Box<dyn Provider + Send> = match stored.kind {
            KIND_CALDAV => Box::new(CalDavProvider::new(creds, label.clone())),
            KIND_ETESYNC => Box::new(EteSyncProvider::new(creds, label.clone())),
            _ => {
                self.as_mut().set_status(QString::from(
                    "OAuth providers (Google / Microsoft) need the sign-in flow to land first.",
                ));
                set_account_state(self.as_mut(), &uuid, "Idle");
                return;
            }
        };

        // Initial in-flight state on the QML thread.
        set_account_state(self.as_mut(), &uuid, "Syncing…");
        self.as_mut()
            .set_status(QString::from(&format!("Syncing {label}…")));

        // Snapshot what the worker thread + completion callback
        // need. CxxQtThread, the runtime handle, the path, the uuid
        // / label strings, and the boxed Send-typed Provider are
        // all Send + 'static.
        let qt_thread = self.as_ref().qt_thread();
        let runtime = self
            .as_mut()
            .rust_mut()
            .runtime
            .as_ref()
            .expect("runtime constructed above")
            .handle()
            .clone();
        let uuid_owned = uuid.clone();
        let label_owned = label.clone();

        // `runtime.spawn` would require the future to be Send, but
        // SyncEngine's pull cycle holds a `rusqlite::Transaction`
        // (which borrows `&Connection`) across `.await` points and
        // `Connection` is !Sync. Drive the future from a dedicated
        // OS thread via `block_on` instead — that polls in place,
        // so the !Sync borrow never crosses thread boundaries.
        // Once the future resolves, queue a callback back onto the
        // QML thread so the Q_PROPERTY updates run with exclusive
        // pinned-mut access.
        std::thread::Builder::new()
            .name(format!("sync:{label}"))
            .spawn(move || {
                let uuid_for_engine = uuid_owned.clone();
                let result = runtime.block_on(async move {
                    let mut engine =
                        SyncEngine::new_for_account(&db_path, provider, uuid_for_engine);
                    engine.sync_now().await
                });
                let _ = qt_thread.queue(move |mut pinned: Pin<&mut qobject::TaskListViewModel>| {
                    match result {
                        Ok(outcome) => {
                            set_account_state(
                                pinned.as_mut(),
                                &uuid_owned,
                                &format!(
                                    "Synced ({}↓ / {}↑)",
                                    outcome.tasks_pulled, outcome.tasks_pushed
                                ),
                            );
                            pinned
                                .as_mut()
                                .set_status(QString::from(&format!("{label_owned}: Done")));
                            refresh_sidebar(pinned.as_mut());
                            pinned.as_mut().reload_active_filter();
                        }
                        Err(e) => {
                            let msg = format!("Sync of {label_owned} failed: {e}");
                            tracing::warn!("{msg}");
                            set_account_state(
                                pinned.as_mut(),
                                &uuid_owned,
                                &format!("Failed: {e}"),
                            );
                            pinned.as_mut().set_status(QString::from(&msg));
                        }
                    }
                });
            })
            .expect("spawn sync worker thread");
    }

    /// Fan out a `sync_account` dispatch to every non-OAuth, non-
    /// local account currently in `self.accounts`. Wired to the
    /// toolbar's manual Sync button + to the auto-sync hooks
    /// triggered by task create / edit / delete.
    pub fn sync_all_accounts(mut self: Pin<&mut Self>) {
        let uuids: Vec<String> = self
            .accounts
            .iter()
            .filter(|a| a.kind == KIND_CALDAV || a.kind == KIND_ETESYNC)
            .map(|a| a.uuid.clone())
            .collect();
        for uuid in uuids {
            self.as_mut().sync_account(QString::from(&uuid));
        }
    }

    /// Repaint the per-list colour. Writes `caldav_lists.cdl_color`
    /// via a transient RW connection then rebuilds the sidebar and
    /// reloads the active filter so the new colour shows up
    /// immediately in the per-row chip + the priority-checkbox
    /// background that keys off it. `color` is i32 ARGB; 0 means
    /// "no custom colour" (the chip falls back to neutral grey).
    pub fn update_list_color(mut self: Pin<&mut Self>, cdl_uuid: QString, color: i32) {
        let uuid = cdl_uuid.to_string();
        if uuid.is_empty() {
            return;
        }
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("Open a database first."));
            return;
        };
        let res = open_rw_conn(&path).and_then(|conn| {
            conn.execute(
                "UPDATE caldav_lists SET cdl_color = ?1 WHERE cdl_uuid = ?2",
                rusqlite::params![color, uuid],
            )
            .map(|_| ())
        });
        if let Err(e) = res {
            self.as_mut()
                .set_status(QString::from(&format!("DB write failed: {e}")));
            return;
        }
        // Refresh sidebar so the new colour drives chip + checkbox
        // backgrounds on the next paint.
        if let Some(db) = &self.db {
            let (labels, ids, kinds, groups, colors) = build_sidebar(db);
            self.as_mut()
                .set_sidebar_labels(string_list_from_iter(labels.iter().map(String::as_str)));
            self.as_mut()
                .set_sidebar_ids(string_list_from_iter(ids.iter().map(String::as_str)));
            let mut kl: QList<i32> = QList::default();
            for k in &kinds {
                kl.append(*k);
            }
            self.as_mut().set_sidebar_account_kinds(kl);
            self.as_mut()
                .set_sidebar_groups(string_list_from_iter(groups.iter().map(String::as_str)));
            let mut cl: QList<i32> = QList::default();
            for c in &colors {
                cl.append(*c);
            }
            self.as_mut().set_sidebar_colors(cl);
        }
        self.as_mut().reload_active_filter();
    }

    /// Create a new calendar on the given account's server, then
    /// run a sync so the local DB picks the row up. The sync also
    /// refreshes the sidebar + reloads the active filter.
    pub fn create_account_calendar(
        mut self: Pin<&mut Self>,
        cda_uuid: QString,
        name: QString,
        color: i32,
    ) {
        let uuid = cda_uuid.to_string();
        let name_s = name.to_string().trim().to_string();
        if name_s.is_empty() {
            self.as_mut()
                .set_status(QString::from("Calendar name is required."));
            return;
        }
        let Some(stored) = self.accounts.iter().find(|a| a.uuid == uuid).cloned() else {
            self.as_mut()
                .set_status(QString::from(&format!("No account with uuid {uuid}.")));
            return;
        };
        let Some(_) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("Open a database first."));
            return;
        };

        if self.runtime.is_none() {
            match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .worker_threads(2)
                .thread_name("tasks-sync")
                .build()
            {
                Ok(rt) => self.as_mut().rust_mut().runtime = Some(rt),
                Err(e) => {
                    self.as_mut()
                        .set_status(QString::from(&format!("Couldn't start runtime: {e}")));
                    return;
                }
            }
        }

        let creds = AccountCredentials::new_password(
            &stored.server,
            &stored.username,
            secrecy::ExposeSecret::expose_secret(&stored.password).to_string(),
        );
        let mut provider: Box<dyn Provider + Send> = match stored.kind {
            KIND_CALDAV => Box::new(CalDavProvider::new(creds, stored.label.clone())),
            KIND_ETESYNC => Box::new(EteSyncProvider::new(creds, stored.label.clone())),
            _ => {
                self.as_mut().set_status(QString::from(
                    "Creating lists on this provider isn't supported yet.",
                ));
                return;
            }
        };

        self.as_mut().set_status(QString::from(&format!(
            "Creating list \"{}\" on {}…",
            name_s, stored.label
        )));

        let create_result = self
            .as_mut()
            .rust_mut()
            .runtime
            .as_ref()
            .expect("runtime constructed above")
            .block_on(async move {
                provider.connect().await?;
                let color_arg = if color == 0 { None } else { Some(color) };
                provider.create_calendar(&name_s, color_arg).await
            });

        match create_result {
            Ok(_cal) => {
                self.as_mut().set_status(QString::from(&format!(
                    "Created list on {}; pulling…",
                    stored.label
                )));
                // Run a sync so the new calendar lands in
                // caldav_lists and the sidebar refreshes.
                self.as_mut().sync_account(QString::from(&uuid));
            }
            Err(e) => {
                let msg = format!("Couldn't create list: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// Create a new task in the open DB with `title`. If the user
    /// is currently viewing a CalDAV-scoped filter
    /// (`caldav:<uuid>`), the new task is stamped into that list;
    /// otherwise it lands as a local task. After creation we
    /// reload the active filter and select the new row so the
    /// user sees it land and can immediately flesh it out via
    /// Edit….
    pub fn add_new_task(mut self: Pin<&mut Self>, title: QString) {
        let title_str = title.to_string();
        let title_trim = title_str.trim();
        if title_trim.is_empty() {
            self.as_mut()
                .set_status(QString::from("Empty task title — nothing created."));
            return;
        }
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't create."));
            return;
        };
        let active = self.active_filter_id.to_string();
        let caldav_uuid = active.strip_prefix("caldav:");
        match tasks_core::create_task(&path, title_trim, now_ms(), caldav_uuid) {
            Ok(new_id) => {
                self.as_mut().reload_active_filter();
                self.as_mut().select_task(new_id);
                self.as_mut()
                    .set_status(QString::from(&format!("Created \"{title_trim}\"")));
                auto_sync_for_task(self.as_mut(), new_id);
            }
            Err(e) => {
                let msg = format!("Couldn't create task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// Mark task `id` as completed (or restore it to active when
    /// `completed = false`). Delegates to `tasks_core::write` and
    /// reloads the active filter so the UI reflects the change
    /// immediately — the filesystem watcher would pick it up on its
    /// next tick anyway, but clicking a checkbox should feel
    /// instantaneous.
    pub fn toggle_task_completion(mut self: Pin<&mut Self>, id: i64, completed: bool) {
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't mark task."));
            return;
        };
        match tasks_core::set_task_completion(&path, id, completed, now_ms()) {
            Ok(true) => {
                self.as_mut().reload_active_filter();
                // reload_active_filter rewrites the status line with a
                // task count; the write feedback is implicit in that.
                // If the selected task was the one we toggled, refresh
                // its detail pane to match.
                if self.selected_id == id {
                    self.as_mut().set_selected_completed(completed);
                }
                auto_sync_for_task(self.as_mut(), id);
            }
            Ok(false) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Task {id} not found")));
            }
            Err(e) => {
                let msg = format!("Couldn't update task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// Soft-delete the task currently shown in the detail pane. No-op
    /// when nothing is selected. After a successful delete the detail
    /// pane clears itself, matching the Android "swipe to delete"
    /// behaviour minus the animation.
    ///
    /// H-6: stamp the deleted id + title onto `last_deleted_*` so the
    /// toast shows an Undo button. The toast clears these via
    /// `clearLastDeleted` when its timer expires; restoring via
    /// `restoreLastDeleted` clears them too.
    pub fn delete_selected_task(mut self: Pin<&mut Self>) {
        let id = self.selected_id;
        if id <= 0 {
            return;
        }
        let title_for_undo = self.selected_title.to_string();
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't delete."));
            return;
        };
        match tasks_core::set_task_deleted(&path, id, now_ms()) {
            Ok(true) => {
                clear_detail_pane(self.as_mut());
                self.as_mut().reload_active_filter();
                self.as_mut().set_last_deleted_id(id);
                self.as_mut().rust_mut().last_deleted_title = title_for_undo.clone();
                let display = if title_for_undo.is_empty() {
                    "task".to_string()
                } else {
                    format!("\u{201C}{}\u{201D}", title_for_undo)
                };
                self.as_mut()
                    .set_status(QString::from(&format!("Deleted {display}. Undo?")));
                // Push the soft-delete to the server so the row's
                // tombstone reaches its CalDAV / EteSync home. The
                // engine's push_dirty path picks up rows whose
                // `tasks.deleted > 0` and issues DELETE.
                auto_sync_for_task(self.as_mut(), id);
            }
            Ok(false) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Task {id} not found")));
            }
            Err(e) => {
                let msg = format!("Couldn't delete task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// H-6: restore the most recently soft-deleted task. No-op when
    /// `last_deleted_id` is 0. On success, reloads the active filter
    /// and selects the restored row so the user immediately sees it
    /// back in the list, and clears the undo state.
    pub fn restore_last_deleted(mut self: Pin<&mut Self>) {
        let id = self.last_deleted_id;
        if id <= 0 {
            return;
        }
        let title = self.last_deleted_title.clone();
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't undo."));
            return;
        };
        match tasks_core::set_task_undeleted(&path, id, now_ms()) {
            Ok(true) => {
                self.as_mut().set_last_deleted_id(0);
                self.as_mut().rust_mut().last_deleted_title.clear();
                self.as_mut().reload_active_filter();
                self.as_mut().select_task(id);
                let display = if title.is_empty() {
                    "task".to_string()
                } else {
                    format!("\u{201C}{}\u{201D}", title)
                };
                self.as_mut()
                    .set_status(QString::from(&format!("Restored {display}.")));
                auto_sync_for_task(self.as_mut(), id);
            }
            Ok(false) => {
                // The row wasn't deleted — clear pinned state so the
                // undo button hides; nothing to do.
                self.as_mut().set_last_deleted_id(0);
                self.as_mut().rust_mut().last_deleted_title.clear();
            }
            Err(e) => {
                let msg = format!("Couldn't undo delete: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// H-6: drop the pinned undo state. Called from the toast
    /// when its hide-timer fires, so the Undo button disappears at
    /// the same time the toast text does.
    pub fn clear_last_deleted(mut self: Pin<&mut Self>) {
        if self.last_deleted_id == 0 {
            return;
        }
        self.as_mut().set_last_deleted_id(0);
        self.as_mut().rust_mut().last_deleted_title.clear();
    }

    pub fn select_task(mut self: Pin<&mut Self>, id: i64) {
        let Some(task) = self.task_cache.iter().find(|t| t.id == id).cloned() else {
            clear_detail_pane(self.as_mut());
            return;
        };

        self.as_mut().set_selected_id(task.id);
        self.as_mut()
            .set_selected_title(QString::from(task.title.as_deref().unwrap_or("")));
        self.as_mut()
            .set_selected_notes(QString::from(task.notes.as_deref().unwrap_or("")));
        self.as_mut()
            .set_selected_due_label(QString::from(&format_due_label(task.due_date)));
        self.as_mut()
            .set_selected_hide_until_label(QString::from(&format_due_label(task.hide_until)));
        self.as_mut().set_selected_priority(task.priority);
        self.as_mut().set_selected_completed(task.is_completed());
        // Humanise the RRULE (FREQ/INTERVAL/BYDAY/UNTIL/COUNT) and
        // mark repeat-from-completion so the user sees the semantic
        // difference from repeat-from-due-date without needing to
        // decode RRULE text.
        let humanized = humanize_rrule(
            task.recurrence.as_deref().unwrap_or(""),
            task.repeat_from == RepeatFrom::COMPLETION_DATE,
        );
        self.as_mut()
            .set_selected_recurrence(QString::from(&humanized));

        // Current CalDAV list assignment (empty for local tasks).
        // Pull the colour alongside so the detail pane's list chip
        // can paint to match `caldav_lists.cdl_color`.
        let (uuid, color) = match &self.db {
            Some(db) => current_caldav_meta_for(db, task.id),
            None => (String::new(), 0),
        };
        self.as_mut()
            .set_selected_caldav_calendar_uuid(QString::from(&uuid));
        self.as_mut().set_selected_caldav_calendar_color(color);

        // Current tag set.
        let task_tag_uids = match &self.db {
            Some(db) => current_tag_uids_for(db, task.id),
            None => Vec::new(),
        };
        self.as_mut().set_selected_tag_uids(string_list_from_iter(
            task_tag_uids.iter().map(String::as_str),
        ));

        // Current alarms (parallel labels/times/types).
        let (alarm_labels, alarm_times, alarm_types) = match &self.db {
            Some(db) => current_alarms_for(db, task.id),
            None => (Vec::new(), Vec::new(), Vec::new()),
        };
        self.as_mut()
            .set_selected_alarm_labels(string_list_from_iter(
                alarm_labels.iter().map(String::as_str),
            ));
        let mut ql_times: QList<i64> = QList::default();
        for t in &alarm_times {
            ql_times.append(*t);
        }
        self.as_mut().set_selected_alarm_times(ql_times);
        let mut ql_types: QList<i32> = QList::default();
        for t in &alarm_types {
            ql_types.append(*t);
        }
        self.as_mut().set_selected_alarm_types(ql_types);

        // Current geofence.
        let (place_uid, arrival, departure) = match &self.db {
            Some(db) => current_geofence_for(db, task.id),
            None => (String::new(), false, false),
        };
        self.as_mut()
            .set_selected_place_uid(QString::from(&place_uid));
        self.as_mut().set_selected_place_arrival(arrival);
        self.as_mut().set_selected_place_departure(departure);

        // Parent picker candidates + current parent.
        let (parent_labels, parent_ids) = match &self.db {
            Some(db) => list_parent_candidates(db, task.id),
            None => (Vec::new(), Vec::new()),
        };
        self.as_mut()
            .set_parent_candidate_labels(string_list_from_iter(
                parent_labels.iter().map(String::as_str),
            ));
        let mut ql_parent_ids: QList<i64> = QList::default();
        for pid in &parent_ids {
            ql_parent_ids.append(*pid);
        }
        self.as_mut().set_parent_candidate_ids(ql_parent_ids);
        self.as_mut().set_selected_parent_id(task.parent);

        // Timer columns → H:MM text for the edit dialog.
        self.as_mut()
            .set_selected_estimated_text(QString::from(&format_duration_hhmm(
                task.estimated_seconds,
            )));
        self.as_mut()
            .set_selected_elapsed_text(QString::from(&format_duration_hhmm(task.elapsed_seconds)));

        // Recurrence raw + repeat_from for the inline RRULE editor.
        self.as_mut()
            .set_selected_recurrence_raw(QString::from(task.recurrence.as_deref().unwrap_or("")));
        self.as_mut().set_selected_repeat_from(task.repeat_from);
    }

    /// Apply edits from the task edit dialog and refresh the view.
    ///
    /// Parses the two date text fields via
    /// `tasks_core::datetime::parse_due_input`; on error we leave
    /// the DB untouched and surface the failure on the status line
    /// so the user can correct the input without losing work.
    pub fn update_selected_task(
        mut self: Pin<&mut Self>,
        title: QString,
        notes: QString,
        due_text: QString,
        hide_until_text: QString,
        priority: i32,
        caldav_uuid: QString,
        tag_uids_list: QStringList,
        alarm_times: QList<i64>,
        alarm_types: QList<i32>,
        place_uid: QString,
        place_arrival: bool,
        place_departure: bool,
        parent_id: i64,
        estimate_text: QString,
        elapsed_text: QString,
        recurrence: QString,
        repeat_from: i32,
    ) {
        let id = self.selected_id;
        if id <= 0 {
            return;
        }
        let Some(path) = self.db_path.clone() else {
            self.as_mut()
                .set_status(QString::from("No database open; can't save edits."));
            return;
        };

        let title_str = title.to_string();
        let notes_str = notes.to_string();
        let caldav_str = caldav_uuid.to_string();
        // QStringList → Vec<String>. cxx-qt-lib's QStringList has
        // no direct iterator, but it converts into QList<QString>
        // cheaply, which does.
        let tag_uids_owned: Vec<String> = {
            let list: QList<QString> = QList::from(&tag_uids_list);
            list.iter().map(|s| s.to_string()).collect()
        };

        // Zip parallel time/type QLists into Vec<(time, type)> for
        // the write helper. If the two arrays disagree in length we
        // trim to the shorter, matching the QML side's guarantee
        // that both are built from the same source.
        let alarm_pairs: Vec<(i64, i32)> = alarm_times
            .iter()
            .zip(alarm_types.iter())
            .map(|(t, ty)| (*t, *ty))
            .collect();
        let place_uid_str = place_uid.to_string();

        let estimated = match parse_duration_input(&estimate_text.to_string()) {
            Ok(s) => s,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Estimate: {msg}")));
                return;
            }
        };
        let recurrence_str = recurrence.to_string();
        let elapsed = match parse_duration_input(&elapsed_text.to_string()) {
            Ok(s) => s,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Elapsed: {msg}")));
                return;
            }
        };
        let due_ms = match parse_due_input(&due_text.to_string()) {
            Ok(ms) => ms,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Due: {msg}")));
                return;
            }
        };
        let hide_ms = match parse_due_input(&hide_until_text.to_string()) {
            Ok(ms) => ms,
            Err(msg) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Hide-until: {msg}")));
                return;
            }
        };

        let edit = tasks_core::TaskEdit {
            title: &title_str,
            notes: &notes_str,
            due_ms,
            hide_until_ms: hide_ms,
            priority,
            // Empty string = "don't touch caldav_tasks" (QML passes
            // the ComboBox's current UUID, which equals the task's
            // existing assignment when unchanged — the helper's
            // UPDATE is idempotent in that case).
            caldav_calendar_uuid: if caldav_str.is_empty() {
                None
            } else {
                Some(caldav_str.as_str())
            },
            tag_uids: Some(&tag_uids_owned),
            alarms: Some(&alarm_pairs),
            geofence: Some(tasks_core::GeofenceEdit {
                place_uid: &place_uid_str,
                arrival: place_arrival,
                departure: place_departure,
            }),
            parent_id: Some(parent_id),
            estimated_seconds: estimated,
            elapsed_seconds: elapsed,
            recurrence: &recurrence_str,
            repeat_from,
        };
        match tasks_core::update_task_fields(&path, id, &edit, now_ms()) {
            Ok(true) => {
                self.as_mut().reload_active_filter();
                // Refresh the detail pane from the cache that
                // `reload_active_filter` just rebuilt, so the user
                // sees their edits reflected without having to
                // re-click the row.
                self.as_mut().select_task(id);
                auto_sync_for_task(self.as_mut(), id);
            }
            Ok(false) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Task {id} not found")));
            }
            Err(e) => {
                let msg = format!("Couldn't save task: {e}");
                tracing::warn!("{msg}");
                self.as_mut().set_status(QString::from(&msg));
            }
        }
    }

    /// Re-query the DB using `self.active_filter_id` and publish the
    /// parallel list arrays. H-4: when `self.search_query` is non-
    /// empty, run the substring search instead of the active filter
    /// — search overrides the filter for the duration of the query
    /// being typed.
    fn reload_active_filter(mut self: Pin<&mut Self>) {
        if self.db.is_none() {
            self.as_mut().clear_list();
            return;
        }

        let now_ms = now_ms();
        let offset = current_local_offset_secs();
        let active_id = self.active_filter_id.to_string();
        let search = self.search_query.clone();
        let prefs = self.preferences.clone();
        // Borrow `db` immutably for the duration of the query, then drop
        // the borrow before any `rust_mut()` call below. `db` lives on
        // `self` (no extra open), so repeated filter navigations reuse
        // the same verified-hash handle.
        let query_result = {
            let Some(ref db) = self.db else {
                unreachable!("db presence checked above");
            };
            if search.is_empty() {
                run_by_filter_id(db, &active_id, now_ms, offset, &prefs)
            } else {
                run_search(db, &search, now_ms, offset, &prefs)
            }
        };
        let tasks = match query_result {
            Ok(t) => t,
            Err(e) => {
                self.as_mut()
                    .set_status(QString::from(&format!("Query failed: {e}")));
                self.as_mut().clear_list();
                return;
            }
        };

        publish_tasks(self.as_mut(), tasks);
    }

    fn clear_list(mut self: Pin<&mut Self>) {
        self.as_mut().set_count(0);
        self.as_mut().set_titles(QStringList::default());
        self.as_mut().set_task_ids(QList::default());
        self.as_mut().set_indents(QList::default());
        self.as_mut().set_completed_flags(QList::default());
        self.as_mut().set_recurring_flags(QList::default());
        self.as_mut().set_due_labels(QStringList::default());
        self.as_mut().set_priorities(QList::default());
        self.as_mut().set_task_tag_summaries(QStringList::default());
        self.as_mut().set_task_tag_uid_lists(QStringList::default());
        self.as_mut().set_task_list_names(QStringList::default());
        self.as_mut().set_task_list_colors(QList::default());
        self.as_mut().rust_mut().task_cache.clear();
    }
}

/// Rebuild every Q_PROPERTY array the Accounts pane binds to
/// from `self.accounts` + `self.account_states`. Called after
/// every add/remove and after each sync_account so the pane
/// reflects the current sync state per row.
fn publish_accounts(mut vm: Pin<&mut qobject::TaskListViewModel>) {
    let snapshot: Vec<StoredAccount> = vm.accounts.clone();
    let states_snapshot: Vec<String> = vm.account_states.clone();
    let mut kinds: QList<i32> = QList::default();
    let mut labels: QList<QString> = QList::default();
    let mut servers: QList<QString> = QList::default();
    let mut users: QList<QString> = QList::default();
    let mut uuids: QList<QString> = QList::default();
    let mut states: QList<QString> = QList::default();
    for (i, a) in snapshot.iter().enumerate() {
        kinds.append(a.kind);
        labels.append(QString::from(&a.label));
        servers.append(QString::from(&a.server));
        users.append(QString::from(&a.username));
        uuids.append(QString::from(&a.uuid));
        let state = states_snapshot.get(i).map(String::as_str).unwrap_or("Idle");
        states.append(QString::from(state));
    }
    vm.as_mut().set_account_kinds(kinds);
    vm.as_mut().set_account_labels(QStringList::from(&labels));
    vm.as_mut().set_account_servers(QStringList::from(&servers));
    vm.as_mut().set_account_usernames(QStringList::from(&users));
    vm.as_mut().set_account_uuids(QStringList::from(&uuids));
    vm.as_mut()
        .set_account_sync_states(QStringList::from(&states));
}

/// Update the per-account sync state for the row whose `cda_uuid`
/// matches `uuid`, then republish the parallel arrays. No-op if
/// the uuid isn't currently in the in-memory accounts list
/// (race with `remove_account`).
fn set_account_state(mut vm: Pin<&mut qobject::TaskListViewModel>, uuid: &str, state: &str) {
    let Some(idx) = vm.accounts.iter().position(|a| a.uuid == uuid) else {
        return;
    };
    {
        let mut inner = vm.as_mut().rust_mut();
        // Pad in case account_states fell behind accounts.len().
        while inner.account_states.len() <= idx {
            inner.account_states.push(String::from("Idle"));
        }
        inner.account_states[idx] = state.to_string();
    }
    publish_accounts(vm);
}

fn publish_tasks(mut vm: Pin<&mut qobject::TaskListViewModel>, tasks: Vec<Task>) {
    let mut task_ids: QList<i64> = QList::default();
    let mut indents: QList<i32> = QList::default();
    let mut completed_flags: QList<bool> = QList::default();
    let mut recurring_flags: QList<bool> = QList::default();
    let mut priorities: QList<i32> = QList::default();

    // `append(&QString)` on QStringList isn't exposed as a public helper;
    // build QList<QString>s alongside and convert at the end.
    let mut title_list: QList<QString> = QList::default();
    let mut due_list: QList<QString> = QList::default();

    // Parent id -> indent depth, cached while iterating so subtasks pick up
    // their parent's indent + 1. The recursive query already sorts with
    // parents before children, but when the prepared-statement fallbacks
    // are used we compute indent from `tasks.parent` on the fly.
    let mut indent_by_id: std::collections::HashMap<i64, i32> = Default::default();

    for t in &tasks {
        title_list.append(QString::from(t.title.as_deref().unwrap_or("")));
        task_ids.append(t.id);
        let indent = if t.parent == 0 {
            0
        } else {
            indent_by_id.get(&t.parent).copied().unwrap_or(0) + 1
        };
        indent_by_id.insert(t.id, indent);
        indents.append(indent);
        completed_flags.append(t.is_completed());
        recurring_flags.append(
            t.recurrence
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false),
        );
        due_list.append(QString::from(&format_due_label(t.due_date)));
        priorities.append(t.priority);
    }
    let titles = QStringList::from(&title_list);
    let due_labels = QStringList::from(&due_list);

    // H-7: per-row tag + list metadata. One query per dimension
    // against the existing DB handle, aggregated to maps keyed by
    // task id, then walked in row order to produce parallel arrays.
    // Single-statement bulk fetches (vs N+1) so a 500-row list
    // costs two extra prepared statements rather than 1000.
    let row_ids: Vec<i64> = tasks.iter().map(|t| t.id).collect();
    let (tag_summary_map, list_meta_map) = {
        match &vm.db {
            Some(db) => (
                fetch_tag_summaries(db, &row_ids),
                fetch_list_meta(db, &row_ids),
            ),
            None => (
                std::collections::HashMap::new(),
                std::collections::HashMap::new(),
            ),
        }
    };
    let mut tag_summary_qlist: QList<QString> = QList::default();
    let mut tag_uid_qlist: QList<QString> = QList::default();
    let mut list_name_qlist: QList<QString> = QList::default();
    let mut list_color_list: QList<i32> = QList::default();
    for t in &tasks {
        match tag_summary_map.get(&t.id) {
            Some((names, uids)) => {
                tag_summary_qlist.append(QString::from(names.as_str()));
                tag_uid_qlist.append(QString::from(uids.as_str()));
            }
            None => {
                tag_summary_qlist.append(QString::default());
                tag_uid_qlist.append(QString::default());
            }
        }
        match list_meta_map.get(&t.id) {
            Some((name, color)) => {
                list_name_qlist.append(QString::from(name.as_str()));
                list_color_list.append(*color);
            }
            None => {
                list_name_qlist.append(QString::default());
                list_color_list.append(0);
            }
        }
    }
    let tag_summaries = QStringList::from(&tag_summary_qlist);
    let tag_uid_lists = QStringList::from(&tag_uid_qlist);
    let list_names = QStringList::from(&list_name_qlist);

    // Two-phase publish to keep QML delegate bindings out of the
    // stale-array race:
    //
    //   1. `set_count(0)` tears down every existing delegate. Each
    //      tear-down reads the old (still consistent) arrays one
    //      last time.
    //   2. Refill the parallel arrays with the new data.
    //   3. `set_count(new_count)` creates fresh delegates which
    //      index into the already-updated arrays.
    //
    // Without this, a filter change from an N-row list to an
    // M-row one (M < N) would leave M+1..N delegates briefly bound
    // to `titles[k>=M]` etc., which resolves to `undefined` and
    // QML emits "Unable to assign [undefined] to QString" warnings.
    // The extra set_count(0) is the price of a clean transition.
    let count = tasks.len() as i32;
    vm.as_mut().set_count(0);
    vm.as_mut().set_titles(titles);
    vm.as_mut().set_task_ids(task_ids);
    vm.as_mut().set_indents(indents);
    vm.as_mut().set_completed_flags(completed_flags);
    vm.as_mut().set_recurring_flags(recurring_flags);
    vm.as_mut().set_due_labels(due_labels);
    vm.as_mut().set_priorities(priorities);
    vm.as_mut().set_task_tag_summaries(tag_summaries);
    vm.as_mut().set_task_tag_uid_lists(tag_uid_lists);
    vm.as_mut().set_task_list_names(list_names);
    vm.as_mut().set_task_list_colors(list_color_list);
    vm.as_mut().set_count(count);
    // The list pane header already prints "N task(s)" — no need to
    // repeat it on every reload, and the previous status chatter
    // overwrote whatever genuine error message was sitting in the
    // status bar.
    vm.as_mut().rust_mut().task_cache = tasks;
}

/// H-7 helper: bulk-fetch the per-task tag info — both the
/// comma-joined display-name summary (used as a tooltip / fallback
/// label) and a comma-joined UID list (used by the list view to look
/// up per-tag colours via the global `tag_uids` / `tag_colors`
/// arrays). Missing tagdata rows fall back to the raw tag_uid for
/// the name so the UI never shows blanks. Tasks with no tags are
/// absent from the map; the caller treats absence as the empty
/// string for both fields.
fn fetch_tag_summaries(
    db: &Database,
    task_ids: &[i64],
) -> std::collections::HashMap<i64, (String, String)> {
    if task_ids.is_empty() {
        return std::collections::HashMap::new();
    }
    // i64 placeholders are safe to splice (no quoting concern); we
    // build the IN clause as a comma-joined integer list.
    let placeholders = task_ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT tags.task, COALESCE(tagdata.name, tags.tag_uid), tags.tag_uid \
         FROM tags LEFT JOIN tagdata ON tags.tag_uid = tagdata.remoteId \
         WHERE tags.task IN ({placeholders}) \
         ORDER BY tags.task, tagdata.name"
    );
    let mut out: std::collections::HashMap<i64, (String, String)> =
        std::collections::HashMap::new();
    let conn = db.connection();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("fetch_tag_summaries: prepare failed: {e}");
            return out;
        }
    };
    let rows = match stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    }) {
        Ok(it) => it,
        Err(e) => {
            tracing::warn!("fetch_tag_summaries: query_map failed: {e}");
            return out;
        }
    };
    for (task_id, name, uid) in rows.flatten() {
        let entry = out.entry(task_id).or_default();
        if entry.0.is_empty() {
            entry.0 = name;
            entry.1 = uid;
        } else {
            entry.0.push_str(", ");
            entry.0.push_str(&name);
            entry.1.push(',');
            entry.1.push_str(&uid);
        }
    }
    out
}

/// H-7 helper: bulk-fetch the per-task CalDAV list `(name, color)`.
/// Tasks not assigned to a list are absent from the map; the caller
/// treats absence as the empty name + colour 0.
fn fetch_list_meta(
    db: &Database,
    task_ids: &[i64],
) -> std::collections::HashMap<i64, (String, i32)> {
    if task_ids.is_empty() {
        return std::collections::HashMap::new();
    }
    let placeholders = task_ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT caldav_tasks.cd_task, \
                COALESCE(caldav_lists.cdl_name, ''), \
                caldav_lists.cdl_color \
         FROM caldav_tasks \
         INNER JOIN caldav_lists ON caldav_tasks.cd_calendar = caldav_lists.cdl_uuid \
         WHERE caldav_tasks.cd_task IN ({placeholders}) \
         AND caldav_tasks.cd_deleted = 0"
    );
    let mut out: std::collections::HashMap<i64, (String, i32)> = std::collections::HashMap::new();
    let conn = db.connection();
    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("fetch_list_meta: prepare failed: {e}");
            return out;
        }
    };
    let rows = match stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i32>(2)?,
        ))
    }) {
        Ok(it) => it,
        Err(e) => {
            tracing::warn!("fetch_list_meta: query_map failed: {e}");
            return out;
        }
    };
    for row in rows.flatten() {
        out.insert(row.0, (row.1, row.2));
    }
    out
}

/// Enumerate the sidebar entries we surface: built-in filters, every CalDAV
/// calendar, then every saved custom filter. Order matches the Android
/// nav drawer's default ordering.
///
/// Errors from reading the `caldav_lists` / `filters` tables are logged
/// (not fatal) so a broken/missing table shows as an incomplete sidebar
/// rather than aborting the whole `openDatabase` flow.
/// Whether an open operation is allowed to bootstrap a missing
/// database file. `openDatabase(path)` from the Browse button only
/// opens what already exists; `openDefaultDatabase` creates-on-miss.
enum OpenMode {
    ReadOnlyOnly,
    CreateIfMissing,
}

/// Shared implementation behind `open_database` and
/// `open_default_database`. Tears down the prior watcher, opens (or
/// initialises) the file, rebuilds the sidebar, and kicks off the
/// initial query. Status-line text and error branches stay the same
/// regardless of which entry point called us.
fn open_at_path(mut vm: Pin<&mut qobject::TaskListViewModel>, path: PathBuf, mode: OpenMode) {
    stop_prior_watcher(vm.as_mut());

    let path_display = path.display().to_string();
    // Open the long-lived handle read-write now that M2+ writes
    // (task edit, account add/remove) and sync writeback all need
    // to mutate the same file. Read-only mode was an M1-era safety
    // net; with the schema-hash check still in place, opening RW
    // here is no riskier and side-steps the SQLITE_READONLY error
    // we were getting when a transient RW connection coincided
    // with the read-only handle.
    let result = match mode {
        OpenMode::ReadOnlyOnly => Database::open_read_only(&path),
        OpenMode::CreateIfMissing => Database::open_or_create_read_write(&path),
    };

    match result {
        Ok(db) => {
            // Auto-create the local-default account + Inbox list
            // before the first sidebar build so a fresh user sees a
            // usable list immediately. Idempotent.
            ensure_local_default_list(&path);
            let (labels, ids, kinds, groups, colors) = build_sidebar(&db);
            vm.as_mut()
                .set_sidebar_labels(string_list_from_iter(labels.iter().map(String::as_str)));
            vm.as_mut()
                .set_sidebar_ids(string_list_from_iter(ids.iter().map(String::as_str)));
            {
                let mut kl: QList<i32> = QList::default();
                for k in &kinds {
                    kl.append(*k);
                }
                vm.as_mut().set_sidebar_account_kinds(kl);
            }
            {
                let mut cl: QList<i32> = QList::default();
                for c in &colors {
                    cl.append(*c);
                }
                vm.as_mut().set_sidebar_colors(cl);
            }
            vm.as_mut()
                .set_sidebar_groups(string_list_from_iter(groups.iter().map(String::as_str)));
            // Edit dialog's CalDAV list picker uses the calendars
            // directly (no built-in filters prepended, no
            // "caldav:" prefix on the UUID).
            let (cal_labels, cal_uuids) = list_caldav_calendars(&db);
            vm.as_mut()
                .set_caldav_calendar_labels(string_list_from_iter(
                    cal_labels.iter().map(String::as_str),
                ));
            vm.as_mut().set_caldav_calendar_uuids(string_list_from_iter(
                cal_uuids.iter().map(String::as_str),
            ));
            let (tag_names, tag_uid_list, tag_color_list) = list_all_tags(&db);
            vm.as_mut()
                .set_tag_labels(string_list_from_iter(tag_names.iter().map(String::as_str)));
            vm.as_mut().set_tag_uids(string_list_from_iter(
                tag_uid_list.iter().map(String::as_str),
            ));
            {
                let mut colors: QList<i32> = QList::default();
                for c in &tag_color_list {
                    colors.append(*c);
                }
                vm.as_mut().set_tag_colors(colors);
            }
            let (place_names, place_uids) = list_all_places(&db);
            vm.as_mut().set_place_labels(string_list_from_iter(
                place_names.iter().map(String::as_str),
            ));
            vm.as_mut()
                .set_place_uids(string_list_from_iter(place_uids.iter().map(String::as_str)));

            // Pull existing caldav_accounts rows into the in-memory
            // accounts list so the Accounts pane shows previously-
            // added (or JSON-imported) accounts and the Sync now
            // button can dispatch against them. Only CalDAV (0) +
            // EteSync (5) are reachable from here today; OAuth
            // providers stay invisible until their sign-in flow
            // lands.
            let loaded = load_password_accounts(&db);
            {
                let mut inner = vm.as_mut().rust_mut();
                inner.account_states = vec![String::from("Idle"); loaded.len()];
                inner.accounts = loaded;
            }
            publish_accounts(vm.as_mut());

            // The local database is exclusively managed by the app
            // and re-opened on every launch — flagging the open in
            // the status bar is just noise. Real failures still set
            // the status text on the error branch below, and the
            // titlebar carries the open path so the user can confirm
            // what's loaded if they want.
            vm.as_mut().set_status(QString::default());
            vm.as_mut()
                .set_db_path_display(QString::from(&path_display));
            {
                let mut inner = vm.as_mut().rust_mut();
                inner.db_path = Some(path.clone());
                inner.db = Some(db);
            }
            vm.as_mut().reload_active_filter();
            start_watcher(vm.as_mut(), path);
        }
        Err(e) => {
            let msg = format!("Couldn't open {path_display}: {e}");
            tracing::warn!("{msg}");
            vm.as_mut().set_status(QString::from(&msg));
            vm.as_mut().set_db_path_display(QString::default());
            vm.as_mut().clear_list();
            // Also blank every per-task detail field + the edit
            // dialog's catalog arrays (tags/places/caldav lists).
            // Otherwise a failed open after an earlier successful
            // open leaves stale values visible in the UI.
            clear_detail_pane(vm.as_mut());
            vm.as_mut().set_tag_labels(QStringList::default());
            vm.as_mut().set_tag_uids(QStringList::default());
            vm.as_mut().set_tag_colors(QList::default());
            vm.as_mut().set_place_labels(QStringList::default());
            vm.as_mut().set_place_uids(QStringList::default());
            vm.as_mut()
                .set_caldav_calendar_labels(QStringList::default());
            vm.as_mut()
                .set_caldav_calendar_uuids(QStringList::default());
            vm.as_mut()
                .set_parent_candidate_labels(QStringList::default());
            vm.as_mut().set_parent_candidate_ids(QList::default());
            let mut inner = vm.as_mut().rust_mut();
            inner.db_path = None;
            inner.db = None;
        }
    }
}

/// Return parallel `(labels, uids)` for every tagdata row. Used by
/// the edit dialog's multi-select tag picker.
fn list_all_tags(db: &Database) -> (Vec<String>, Vec<String>, Vec<i32>) {
    let mut labels = Vec::new();
    let mut uids = Vec::new();
    let mut colors = Vec::new();
    let Ok(mut stmt) = db.connection().prepare(
        "SELECT remoteId, name, COALESCE(color, 0) FROM tagdata \
         WHERE remoteId IS NOT NULL AND name IS NOT NULL \
         ORDER BY td_order, name",
    ) else {
        return (labels, uids, colors);
    };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i32>(2)?,
        ))
    });
    if let Ok(rows) = rows {
        for (uid, name, color) in rows.flatten() {
            uids.push(uid);
            labels.push(name);
            colors.push(color);
        }
    }
    (labels, uids, colors)
}

/// Return parallel `(labels, ids)` for every non-deleted task,
/// excluding `exclude_id` (the task currently being edited, so the
/// picker never offers the task itself as its own parent). Sorted
/// by title for a predictable dropdown.
fn list_parent_candidates(db: &Database, exclude_id: i64) -> (Vec<String>, Vec<i64>) {
    let mut labels = Vec::new();
    let mut ids = Vec::new();
    let Ok(mut stmt) = db.connection().prepare(
        "SELECT _id, title FROM tasks \
         WHERE deleted = 0 AND _id != ?1 \
         ORDER BY COALESCE(UPPER(title), ''), _id",
    ) else {
        return (labels, ids);
    };
    let rows = stmt.query_map([exclude_id], |r| {
        let id: i64 = r.get(0)?;
        let title: Option<String> = r.get(1)?;
        Ok((id, title.unwrap_or_default()))
    });
    if let Ok(rows) = rows {
        for (id, title) in rows.flatten() {
            ids.push(id);
            labels.push(title);
        }
    }
    (labels, ids)
}

/// Return parallel `(labels, uids)` for every row in `places`.
fn list_all_places(db: &Database) -> (Vec<String>, Vec<String>) {
    let mut labels = Vec::new();
    let mut uids = Vec::new();
    let Ok(mut stmt) = db.connection().prepare(
        "SELECT uid, name FROM places \
         WHERE uid IS NOT NULL AND name IS NOT NULL \
         ORDER BY place_order, name",
    ) else {
        return (labels, uids);
    };
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)));
    if let Ok(rows) = rows {
        for (uid, name) in rows.flatten() {
            uids.push(uid);
            labels.push(name);
        }
    }
    (labels, uids)
}

/// Fetch the geofence row for `task_id`, returning
/// `(place_uid, arrival, departure)`. Empty `place_uid` = no row.
/// If a task has multiple geofences (rare; schema allows it) we
/// pick the first by rowid.
fn current_geofence_for(db: &Database, task_id: i64) -> (String, bool, bool) {
    db.connection()
        .query_row(
            "SELECT place, arrival, departure FROM geofences \
             WHERE task = ?1 ORDER BY geofence_id LIMIT 1",
            [task_id],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?.unwrap_or_default(),
                    r.get::<_, i32>(1)? != 0,
                    r.get::<_, i32>(2)? != 0,
                ))
            },
        )
        .unwrap_or_default()
}

/// Read the alarms attached to `task_id`. Returns three parallel
/// vectors suitable for the bridge's QStringList/QList Q_PROPERTYs.
fn current_alarms_for(db: &Database, task_id: i64) -> (Vec<String>, Vec<i64>, Vec<i32>) {
    let mut labels = Vec::new();
    let mut times = Vec::new();
    let mut types = Vec::new();
    let Ok(mut stmt) = db
        .connection()
        .prepare("SELECT time, type FROM alarms WHERE task = ?1 ORDER BY time")
    else {
        return (labels, times, types);
    };
    let rows = stmt.query_map([task_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i32>(1)?))
    });
    if let Ok(rows) = rows {
        for (time, alarm_type) in rows.flatten() {
            labels.push(describe_alarm(alarm_type, time));
            times.push(time);
            types.push(alarm_type);
        }
    }
    (labels, times, types)
}

/// Fetch the tag UIDs attached to `task_id` via the `tags` join.
fn current_tag_uids_for(db: &Database, task_id: i64) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(mut stmt) = db
        .connection()
        .prepare("SELECT tag_uid FROM tags WHERE task = ?1 AND tag_uid IS NOT NULL")
    else {
        return out;
    };
    if let Ok(rows) = stmt.query_map([task_id], |r| r.get::<_, String>(0)) {
        for uid in rows.flatten() {
            out.push(uid);
        }
    }
    out
}

/// Return parallel `(labels, uuids)` for every CalDAV calendar. Used
/// by the edit dialog's list picker; `build_sidebar` has a richer
/// shape because its output also includes the built-in filter IDs.
fn list_caldav_calendars(db: &Database) -> (Vec<String>, Vec<String>) {
    let mut labels = Vec::new();
    let mut uuids = Vec::new();
    let Ok(mut stmt) = db
        .connection()
        .prepare("SELECT * FROM caldav_lists ORDER BY cdl_order, cdl_name")
    else {
        return (labels, uuids);
    };
    let Ok(rows) = stmt.query_map([], CaldavCalendar::from_row) else {
        return (labels, uuids);
    };
    for row in rows.flatten() {
        if let (Some(name), Some(uuid)) = (row.name, row.uuid) {
            labels.push(name);
            uuids.push(uuid);
        }
    }
    (labels, uuids)
}

/// Look up the CalDAV calendar `(uuid, colour)` assigned to
/// `task_id`. Returns `("", 0)` when the task has no
/// `caldav_tasks` row (local-only) or no matching `caldav_lists`
/// row. The colour is the stored `cdl_color` ARGB i32.
fn current_caldav_meta_for(db: &Database, task_id: i64) -> (String, i32) {
    let row: Option<(Option<String>, Option<i32>)> = db
        .connection()
        .query_row(
            "SELECT caldav_tasks.cd_calendar, caldav_lists.cdl_color \
             FROM caldav_tasks \
             LEFT JOIN caldav_lists \
                ON caldav_tasks.cd_calendar = caldav_lists.cdl_uuid \
             WHERE caldav_tasks.cd_task = ?1 \
             LIMIT 1",
            [task_id],
            |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i32>>(1)?)),
        )
        .ok();
    match row {
        Some((Some(uuid), color)) => (uuid, color.unwrap_or(0)),
        _ => (String::new(), 0),
    }
}

/// Load every password-auth `caldav_accounts` row (kinds 0 = CALDAV
/// and 5 = ETEBASE) into the bridge's in-memory `accounts` list so
/// the Accounts pane reflects whatever the DB carries — both rows
/// the user added in a prior session and rows brought in by the
/// JSON-import path. OAuth providers stay invisible here until
/// their sign-in flow lands; their tokens won't be in `cda_password`
/// anyway.
fn load_password_accounts(db: &Database) -> Vec<StoredAccount> {
    let mut out = Vec::new();
    let Ok(mut stmt) = db.connection().prepare(
        "SELECT cda_uuid, cda_name, cda_url, cda_username, cda_password, cda_account_type \
         FROM caldav_accounts \
         WHERE cda_account_type IN (0, 5) \
         ORDER BY cda_account_type, cda_name",
    ) else {
        return out;
    };
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, i32>(5)?,
        ))
    });
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            let (uuid, name, url, username, password, kind_in_db) = row;
            let Some(uuid) = uuid else { continue };
            // Map cda_account_type back onto the bridge's KIND_*
            // integers (which match `tasks_sync::ProviderKind` for
            // QML, not the cda_account_type column).
            let kind = match kind_in_db {
                0 => KIND_CALDAV,
                5 => KIND_ETESYNC,
                _ => continue,
            };
            out.push(StoredAccount {
                uuid,
                kind,
                label: name.unwrap_or_default(),
                server: url.unwrap_or_default(),
                username: username.unwrap_or_default(),
                password: SecretString::from(password.unwrap_or_default()),
            });
        }
    }
    out
}

/// Output bundle for `build_sidebar`. Aliased so the function
/// signature doesn't trip `clippy::type_complexity`; the five
/// vectors are the same five Q_PROPERTYs the QML side reads:
/// labels / ids / account_kinds / groups / colors.
type SidebarRows = (Vec<String>, Vec<String>, Vec<i32>, Vec<String>, Vec<i32>);

fn build_sidebar(db: &Database) -> SidebarRows {
    let mut labels = vec![
        "All active".to_string(),
        "Today".to_string(),
        "Recently modified".to_string(),
    ];
    let mut ids = vec![
        FILTER_ALL.to_string(),
        FILTER_TODAY.to_string(),
        FILTER_RECENT.to_string(),
    ];
    // Group key for each row, parallel to `ids`. Built-ins share
    // "filters_builtin"; saved filters share "saved"; account
    // header + the lists hanging off it share `account:<uuid>`.
    let mut groups: Vec<String> = vec![
        "filters_builtin".to_string(),
        "filters_builtin".to_string(),
        "filters_builtin".to_string(),
    ];
    // Per-row i32 ARGB colour. Non-list rows (built-ins, account
    // headers, saved filters) carry 0 — the QML side treats that
    // as "no swatch".
    let mut colors: Vec<i32> = vec![0, 0, 0];
    // `kinds[i]` carries:
    //   -1 for built-in / saved filters
    //   `cda_account_type` (0/2/3/4/5/6/7) for `caldav:` rows AND
    //   for the synthetic `account:<uuid>` headers emitted below
    // The QML side looks at the id prefix to decide whether the row
    // is a section header (no selection / right-click menu) or a
    // selectable list filter.
    let mut kinds: Vec<i32> = vec![-1, -1, -1];

    // Walk caldav_accounts once, then for each emit a synthetic
    // header followed by every cdl row whose cdl_account matches.
    // Empty accounts still produce a header so the user can right-
    // click "New list" on them. `cda_collapsed` is honoured by the
    // QML side via the existing `collapsedGroups` map (keyed by
    // account uuid); we just provide the rows.
    let mut accounts: Vec<(String, String, i32)> = Vec::new(); // (uuid, label, type)
    if let Ok(mut stmt) = db.connection().prepare(
        "SELECT cda_uuid, cda_name, cda_account_type \
         FROM caldav_accounts \
         WHERE cda_uuid IS NOT NULL \
         ORDER BY cda_account_type, cda_name",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i32>(2)?,
            ))
        }) {
            for row in rows.flatten() {
                if let (Some(uuid), name) = (row.0, row.1) {
                    accounts.push((uuid, name.unwrap_or_default(), row.2));
                }
            }
        }
    }
    // Lookup table: cdl_account → list of (cdl_uuid, cdl_name, cdl_color).
    let mut lists_by_account: std::collections::HashMap<String, Vec<(String, String, i32)>> =
        std::collections::HashMap::new();
    if let Ok(mut stmt) = db.connection().prepare(
        "SELECT cdl_uuid, cdl_name, cdl_account, COALESCE(cdl_color, 0) \
         FROM caldav_lists \
         WHERE cdl_account IS NOT NULL AND cdl_uuid IS NOT NULL \
         ORDER BY cdl_order, cdl_name",
    ) {
        if let Ok(rows) = stmt.query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i32>(3)?,
            ))
        }) {
            for row in rows.flatten() {
                if let (Some(uuid), name, Some(account), color) = (row.0, row.1, row.2, row.3) {
                    lists_by_account.entry(account).or_default().push((
                        uuid,
                        name.unwrap_or_default(),
                        color,
                    ));
                }
            }
        }
    }
    for (account_uuid, account_name, account_type) in &accounts {
        let account_group = format!("account:{account_uuid}");
        // Header row. The id prefix `account:` is what tells the QML
        // "this is a section header — render with the account label,
        // toggle collapse on click, show a right-click menu."
        labels.push(if account_name.is_empty() {
            "(unnamed account)".to_string()
        } else {
            account_name.clone()
        });
        ids.push(account_group.clone());
        kinds.push(*account_type);
        groups.push(account_group.clone());
        colors.push(0);
        // Lists belonging to this account.
        if let Some(rows) = lists_by_account.get(account_uuid) {
            for (cdl_uuid, cdl_name, cdl_color) in rows {
                labels.push(if cdl_name.is_empty() {
                    cdl_uuid.clone()
                } else {
                    cdl_name.clone()
                });
                ids.push(format!("caldav:{cdl_uuid}"));
                kinds.push(*account_type);
                groups.push(account_group.clone());
                colors.push(*cdl_color);
            }
        }
    }

    match db
        .connection()
        .prepare("SELECT * FROM filters ORDER BY f_order, title")
    {
        Ok(mut stmt) => match stmt.query_map([], CustomFilter::from_row) {
            Ok(rows) => {
                for row in rows {
                    match row {
                        Ok(f) => {
                            if let Some(title) = f.title {
                                labels.push(title);
                                ids.push(format!("filter:{}", f.id));
                                kinds.push(-1);
                                groups.push("saved".to_string());
                                colors.push(0);
                            }
                        }
                        Err(e) => tracing::warn!("filters row decode failed: {e}"),
                    }
                }
            }
            Err(e) => tracing::warn!("filters query_map failed: {e}"),
        },
        Err(e) => tracing::warn!("filters prepare failed: {e}"),
    }

    (labels, ids, kinds, groups, colors)
}

/// Make sure a Local account + a default "Inbox" list exist on the
/// open DB. `caldav_accounts.cda_uuid` and `caldav_lists.cdl_uuid`
/// are plain TEXT columns (no UNIQUE constraint), so `INSERT OR
/// IGNORE` won't de-dupe — every restart would inflate the
/// sidebar with another copy. Read-then-conditional-INSERT keeps
/// the rows singleton.
fn ensure_local_default_list(path: &std::path::Path) {
    let Ok(conn) = open_rw_conn(path) else {
        tracing::warn!("ensure_local_default_list: couldn't open RW conn");
        return;
    };
    let account_present: bool = conn
        .query_row(
            "SELECT 1 FROM caldav_accounts WHERE cda_uuid = 'local-default' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !account_present {
        // account_type = 2 == AccountType::LOCAL.
        let _ = conn.execute(
            "INSERT INTO caldav_accounts \
             (cda_uuid, cda_name, cda_url, cda_username, cda_password, cda_error, \
              cda_account_type, cda_collapsed, cda_server_type, cda_last_sync) \
             VALUES ('local-default', 'Local', NULL, NULL, NULL, NULL, 2, 0, -1, 0)",
            [],
        );
    }
    let list_present: bool = conn
        .query_row(
            "SELECT 1 FROM caldav_lists WHERE cdl_uuid = 'local-inbox' LIMIT 1",
            [],
            |_| Ok(()),
        )
        .is_ok();
    if !list_present {
        let _ = conn.execute(
            "INSERT INTO caldav_lists \
             (cdl_uuid, cdl_account, cdl_name, cdl_color, cdl_url, cdl_access, \
              cdl_ctag, cdl_order, cdl_last_sync) \
             VALUES ('local-inbox', 'local-default', 'Inbox', 0, NULL, 1, NULL, 0, 0)",
            [],
        );
    }
}

/// Snapshot the persistable subset of the view model and write it
/// to disk. Called whenever an updateXxx invokable changes a
/// preference; failures are logged but never propagated (the
/// session keeps running on the in-memory copy).
/// Open a short-lived read-write connection to the SQLite file at
/// `path`. Mirrors `tasks_core::write::open_rw` (which is private)
/// — the bridge's `Database` handle is intentionally read-only so
/// the query path can't accidentally mutate; writes that go around
/// `tasks-core`'s helpers (currently just our `caldav_accounts`
/// add / remove) get their own brief RW connection that closes
/// once the call returns.
/// Recompute the sidebar from the open DB and republish every
/// parallel Q_PROPERTY the QML side reads. Factored out so the
/// background sync completion path (which queues a callback onto
/// the QML thread) can call it through a single function pointer
/// rather than duplicating the five `set_xxx` calls.
/// Look up the syncable owner of `task_id`. Returns `None` when:
/// * The task has no `caldav_tasks` row (purely local, no remote
///   to push to).
/// * The owning account's `cda_account_type` is LOCAL (2) — the
///   default `local-default` Inbox falls into this bucket.
/// * Any of the joins miss (orphaned row, etc.).
///
/// Callers that get `Some(uuid)` dispatch a `sync_account`
/// against it so the just-mutated row reaches the server without
/// waiting for the user to hit the toolbar's manual Sync button.
fn syncable_account_for_task(db: &Database, task_id: i64) -> Option<String> {
    db.connection()
        .query_row(
            "SELECT cl.cdl_account FROM caldav_tasks ct \
             JOIN caldav_lists cl ON cl.cdl_uuid = ct.cd_calendar \
             JOIN caldav_accounts ca ON ca.cda_uuid = cl.cdl_account \
             WHERE ct.cd_task = ?1 AND ca.cda_account_type != 2 \
             LIMIT 1",
            [task_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
}

/// Wrapper that runs `syncable_account_for_task` against the
/// view model's open DB and dispatches `sync_account` if a
/// non-local owner exists. No-op for tasks that belong to the
/// local-default account (the desktop's built-in Inbox) — those
/// don't need a network round-trip.
fn auto_sync_for_task(mut vm: Pin<&mut qobject::TaskListViewModel>, task_id: i64) {
    let uuid = {
        let r = vm.as_ref();
        let inner = r.rust();
        inner
            .db
            .as_ref()
            .and_then(|db| syncable_account_for_task(db, task_id))
    };
    if let Some(uuid) = uuid {
        vm.as_mut().sync_account(QString::from(&uuid));
    }
}

fn refresh_sidebar(mut vm: Pin<&mut qobject::TaskListViewModel>) {
    // Compute the new arrays inside a scope that holds the
    // immutable Rust borrow on the view model, then drop the
    // borrow before the cxx-qt `set_*` methods take Pin<&mut Self>.
    // Without the explicit binding, `vm.as_ref()` is a temporary
    // that dies before `build_sidebar` finishes using it.
    let computed = {
        let r = vm.as_ref();
        let inner = r.rust();
        inner.db.as_ref().map(build_sidebar)
    };
    let Some((labels, ids, kinds, groups, colors)) = computed else {
        return;
    };
    let labels_qsl = string_list_from_iter(labels.iter().map(String::as_str));
    let ids_qsl = string_list_from_iter(ids.iter().map(String::as_str));
    let groups_qsl = string_list_from_iter(groups.iter().map(String::as_str));
    let mut kl: QList<i32> = QList::default();
    for k in &kinds {
        kl.append(*k);
    }
    let mut cl: QList<i32> = QList::default();
    for c in &colors {
        cl.append(*c);
    }
    vm.as_mut().set_sidebar_labels(labels_qsl);
    vm.as_mut().set_sidebar_ids(ids_qsl);
    vm.as_mut().set_sidebar_account_kinds(kl);
    vm.as_mut().set_sidebar_groups(groups_qsl);
    vm.as_mut().set_sidebar_colors(cl);
}

fn open_rw_conn(path: &std::path::Path) -> rusqlite::Result<rusqlite::Connection> {
    let flags =
        rusqlite::OpenFlags::SQLITE_OPEN_READ_WRITE | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = rusqlite::Connection::open_with_flags(path, flags)?;
    conn.busy_timeout(std::time::Duration::from_millis(1_000))?;
    Ok(conn)
}

fn persist_prefs(vm: &qobject::TaskListViewModel) {
    let prefs = crate::preferences::Preferences {
        theme_mode: vm.theme_mode,
        sort_mode: vm.pref_sort_mode,
        sort_ascending: vm.pref_sort_ascending,
        show_completed: vm.pref_show_completed,
        show_hidden: vm.pref_show_hidden,
        completed_at_bottom: vm.pref_completed_at_bottom,
    };
    prefs.save();
}

fn string_list_from_iter<'a>(iter: impl Iterator<Item = &'a str>) -> QStringList {
    let mut list: QList<QString> = QList::default();
    for s in iter {
        list.append(QString::from(s));
    }
    QStringList::from(&list)
}

/// Tell any previously-running watcher thread to exit. The thread
/// observes the shared atomic on its next 500 ms tick and returns.
fn stop_prior_watcher(mut vm: Pin<&mut qobject::TaskListViewModel>) {
    if let Some(stop) = vm.as_mut().rust_mut().watcher_stop.take() {
        stop.store(true, Ordering::Relaxed);
    }
}

/// Spawn a background thread that watches the directory containing
/// `path` and queues a `reload_active_filter` on the Qt thread when
/// the debouncer fires.
///
/// The thread takes its own `DatabaseWatcher` so the `Receiver`
/// stays thread-local and the Debouncer is kept alive for the watch
/// duration. A shared `AtomicBool` lets `open_database` terminate
/// the prior watcher when a new file is selected.
fn start_watcher(mut vm: Pin<&mut qobject::TaskListViewModel>, path: PathBuf) {
    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = Arc::clone(&stop);
    vm.as_mut().rust_mut().watcher_stop = Some(stop);

    let qt_thread = vm.as_ref().qt_thread();

    std::thread::spawn(move || {
        let watcher = match DatabaseWatcher::start(&path) {
            Ok(w) => w,
            Err(e) => {
                tracing::warn!("DatabaseWatcher::start failed for {:?}: {e}", path);
                return;
            }
        };
        tracing::info!("watching {} for changes", path.display());
        loop {
            if stop_thread.load(Ordering::Relaxed) {
                tracing::debug!("watcher thread stopping for {:?}", path);
                return;
            }
            match watcher.events.recv_timeout(Duration::from_millis(500)) {
                Ok(_event) => {
                    if let Err(e) = qt_thread.queue(|pinned| {
                        pinned.reload_active_filter();
                    }) {
                        tracing::warn!("couldn't queue reload on Qt thread: {e}");
                        return;
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => {
                    tracing::debug!("watcher channel closed; thread exiting");
                    return;
                }
            }
        }
    });
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Current process-local UTC offset in seconds, east-positive. Used to
/// anchor `FILTER_TODAY` to local midnight rather than UTC midnight.
/// Delegates to Qt's `QDateTime::offsetFromUtc()` so Qt's own timezone
/// database resolves DST transitions — we avoid pulling in a parallel
/// time library.
fn current_local_offset_secs() -> i32 {
    QDateTime::current_date_time().offset_from_utc()
}

/// Reset every `selected_*` Q_PROPERTY to its empty/default value.
/// Shared between the "unknown id" branch of `select_task` and the
/// post-delete cleanup so a future new field only has to be added
/// in one place.
fn clear_detail_pane(mut vm: Pin<&mut qobject::TaskListViewModel>) {
    vm.as_mut().set_selected_id(0);
    vm.as_mut().set_selected_title(QString::default());
    vm.as_mut().set_selected_notes(QString::default());
    vm.as_mut().set_selected_due_label(QString::default());
    vm.as_mut()
        .set_selected_hide_until_label(QString::default());
    vm.as_mut().set_selected_priority(Priority::NONE);
    vm.as_mut().set_selected_completed(false);
    vm.as_mut().set_selected_recurrence(QString::default());
    vm.as_mut()
        .set_selected_caldav_calendar_uuid(QString::default());
    vm.as_mut().set_selected_caldav_calendar_color(0);
    vm.as_mut().set_selected_tag_uids(QStringList::default());
    vm.as_mut()
        .set_selected_alarm_labels(QStringList::default());
    vm.as_mut().set_selected_alarm_times(QList::default());
    vm.as_mut().set_selected_alarm_types(QList::default());
    vm.as_mut().set_selected_place_uid(QString::default());
    vm.as_mut().set_selected_place_arrival(false);
    vm.as_mut().set_selected_place_departure(false);
    vm.as_mut()
        .set_parent_candidate_labels(QStringList::default());
    vm.as_mut().set_parent_candidate_ids(QList::default());
    vm.as_mut().set_selected_parent_id(0);
    vm.as_mut().set_selected_estimated_text(QString::default());
    vm.as_mut().set_selected_elapsed_text(QString::default());
    vm.as_mut().set_selected_recurrence_raw(QString::default());
    vm.as_mut().set_selected_repeat_from(0);
}
