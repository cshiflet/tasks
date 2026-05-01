//! JSON backup import.
//!
//! Reads the JSON format produced by
//! `app/src/main/java/org/tasks/backup/TasksJsonExporter.kt` and
//! inserts every entity into a target SQLite file. Used to seed a
//! fresh desktop database from an Android export.
//!
//! Known limitations (explicit, documented):
//!
//! * **Attachments and user-activity comments are skipped.** The
//!   export carries their metadata but the file contents live
//!   elsewhere (URI references); restoring those faithfully is its
//!   own packaging concern.
//! * **Task list metadata and Astrid-era legacy locations** are
//!   skipped for the same reason — rarely populated on a modern
//!   install.
//!
//! Parent-child subtask links *are* restored, indirectly: Kotlin marks
//! `Task.parent: Long` as `@Transient` so the JSON doesn't carry it,
//! but the per-task `caldavTasks.remoteParent` field points at the
//! owning task's CalDAV UID (`caldavTasks.remoteId`). After the
//! main insert pass we walk that graph and stamp `tasks.parent`.
//!
//! Import is transactional: either every row lands or nothing
//! does. Existing rows at the same primary key are *replaced*
//! (`INSERT OR REPLACE`), so re-importing the same backup is
//! idempotent.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rusqlite::{params, Connection};
use serde::Deserialize;

use crate::error::{CoreError, Result};

/// Hard ceiling on the size of a JSON backup the importer will
/// read. Real Android exports are measured in single-digit MiB;
/// 200 MiB is generous headroom. A hostile or broken file larger
/// than this is rejected before we allocate any buffer — defends
/// H-4 (memory-exhaustion DoS via an adversarial backup).
const MAX_BACKUP_BYTES: u64 = 200 * 1024 * 1024;

/// Counts of rows inserted, keyed by entity type.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportStats {
    pub tasks: usize,
    pub alarms: usize,
    pub geofences: usize,
    pub tags: usize,
    pub caldav_tasks: usize,
    pub places: usize,
    pub tag_data: usize,
    pub filters: usize,
    pub caldav_accounts: usize,
    pub caldav_calendars: usize,
    pub skipped_attachments: usize,
    pub skipped_comments: usize,
    /// Number of tasks whose `parent` was set by the post-insert
    /// re-link pass. Orphans (remoteParent points at a UID not in
    /// this backup) stay at `parent = 0` and don't contribute.
    pub subtask_links: usize,
}

/// Root of the Android JSON backup. Everything under `data` is what
/// we materialise; `version` + `timestamp` are surfaced to callers
/// so they can warn on extreme drift but we don't gate the import
/// on them (old backups in unsupported schemas will surface as
/// column-mismatch errors on INSERT instead, with a clearer message
/// than "schema-version mismatch" would give).
#[derive(Debug, Deserialize)]
struct BackupRoot {
    #[serde(default)]
    version: i64,
    #[serde(default)]
    timestamp: i64,
    data: BackupData,
}

#[derive(Debug, Deserialize)]
struct BackupData {
    #[serde(default)]
    tasks: Vec<TaskBundle>,
    #[serde(default)]
    places: Vec<PlaceJson>,
    /// Counter-intuitively, the top-level `tags` key holds
    /// `TagData` rows (tag *definitions*). The per-task `tags` list
    /// nested under each TaskBundle is the Tag *join* rows.
    #[serde(default)]
    tags: Vec<TagDataJson>,
    #[serde(default)]
    filters: Vec<FilterJson>,
    #[serde(default, rename = "caldavAccounts")]
    caldav_accounts: Vec<CaldavAccountJson>,
    #[serde(default, rename = "caldavCalendars")]
    caldav_calendars: Vec<CaldavCalendarJson>,
}

/// One entry under `data.tasks[]` — the task plus its per-task
/// children. Matches `org.tasks.backup.TaskBackup` in the exporter.
#[derive(Debug, Deserialize)]
struct TaskBundle {
    task: TaskJson,
    #[serde(default)]
    alarms: Vec<AlarmJson>,
    #[serde(default)]
    geofences: Vec<GeofenceJson>,
    #[serde(default)]
    tags: Vec<TagJson>,
    #[serde(default, rename = "caldavTasks")]
    caldav_tasks: Vec<CaldavTaskJson>,
    // Intentionally unused (see module docs):
    #[serde(default)]
    attachments: Vec<serde_json::Value>,
    #[serde(default)]
    comments: Vec<serde_json::Value>,
}

// Each entity's JSON mirror uses `rename_all = "camelCase"` to
// match Kotlin's default `@Serializable` key derivation (which
// keeps the Kotlin property name). `@Transient` Kotlin fields
// (like Task.id / Task.parent) are absent from the JSON; we rely
// on `#[serde(default)]` — but for the types where those fields
// are _not_ transient we deserialize them normally.

#[derive(Debug, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct TaskJson {
    // `id` and `parent` are @Transient in Kotlin — never present.
    title: Option<String>,
    priority: i32,
    due_date: i64,
    hide_until: i64,
    creation_date: i64,
    modification_date: i64,
    completion_date: i64,
    deletion_date: i64,
    notes: Option<String>,
    estimated_seconds: i32,
    elapsed_seconds: i32,
    timer_start: i64,
    ring_flags: i32,
    reminder_last: i64,
    recurrence: Option<String>,
    repeat_from: i32,
    // `calendarURI` in Kotlin → "calendarURI" in JSON (Kotlin keeps
    // the property name verbatim regardless of rename_all).
    #[serde(rename = "calendarURI")]
    calendar_uri: Option<String>,
    remote_id: Option<String>,
    is_collapsed: bool,
    order: Option<i64>,
    read_only: bool,
}

impl Default for TaskJson {
    fn default() -> Self {
        TaskJson {
            title: None,
            priority: 3, // Priority::NONE
            due_date: 0,
            hide_until: 0,
            creation_date: 0,
            modification_date: 0,
            completion_date: 0,
            deletion_date: 0,
            notes: None,
            estimated_seconds: 0,
            elapsed_seconds: 0,
            timer_start: 0,
            ring_flags: 0,
            reminder_last: 0,
            recurrence: None,
            repeat_from: 0,
            calendar_uri: None,
            remote_id: None,
            is_collapsed: false,
            order: None,
            read_only: false,
        }
    }
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct AlarmJson {
    // id / task are @Transient — we link to the newly-inserted task's
    // rowid, not the original.
    time: i64,
    #[serde(rename = "type")]
    alarm_type: i32,
    repeat: i32,
    interval: i64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct GeofenceJson {
    place: Option<String>,
    arrival: bool,
    departure: bool,
    // `radius` is in some exports; ignore if missing.
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct TagJson {
    name: Option<String>,
    tag_uid: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CaldavTaskJson {
    calendar: Option<String>,
    remote_id: Option<String>,
    #[serde(rename = "object")]
    object_name: Option<String>,
    etag: Option<String>,
    last_sync: i64,
    deleted: i64,
    remote_parent: Option<String>,
    #[serde(default, rename = "gt_moved")]
    is_moved: bool,
    #[serde(default, rename = "gt_remote_order")]
    remote_order: i64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct PlaceJson {
    uid: Option<String>,
    name: Option<String>,
    address: Option<String>,
    phone: Option<String>,
    url: Option<String>,
    latitude: f64,
    longitude: f64,
    color: i32,
    icon: Option<String>,
    order: i32,
    #[serde(default = "default_radius")]
    radius: i32,
}

fn default_radius() -> i32 {
    250
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct TagDataJson {
    remote_id: Option<String>,
    name: Option<String>,
    color: Option<i32>,
    tag_ordering: Option<String>,
    icon: Option<String>,
    order: i32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct FilterJson {
    title: Option<String>,
    sql: Option<String>,
    values: Option<String>,
    criterion: Option<String>,
    color: Option<i32>,
    icon: Option<String>,
    order: i32,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CaldavAccountJson {
    uuid: Option<String>,
    name: Option<String>,
    url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    error: Option<String>,
    account_type: i32,
    is_collapsed: bool,
    server_type: i32,
    last_sync: i64,
}

#[derive(Debug, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
struct CaldavCalendarJson {
    account: Option<String>,
    uuid: Option<String>,
    name: Option<String>,
    color: i32,
    ctag: Option<String>,
    url: Option<String>,
    icon: Option<String>,
    order: i32,
    access: i32,
    last_sync: i64,
}

/// Strip the stray leading slash a QML FileDialog leaves in front
/// of a Windows drive letter after the `file://` prefix has been
/// removed (e.g. `/C:/Users/...` → `C:/Users/...`). No-op on other
/// inputs. Kept here defensively in addition to the QML-side
/// cleanup so any future caller — tests, a bespoke CLI path,
/// another platform dialog — can't repeat the same mistake.
fn normalize_windows_path(input: &Path) -> std::path::PathBuf {
    let s = match input.to_str() {
        Some(s) => s,
        None => return input.to_path_buf(),
    };
    let bytes = s.as_bytes();
    // "/X:..." where X is an ASCII letter.
    if bytes.len() >= 3 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        return std::path::PathBuf::from(&s[1..]);
    }
    input.to_path_buf()
}

/// Read a Tasks.org JSON backup from `source_path` and apply it to
/// the database at `target_path`. The target file must already
/// exist — call `Database::open_or_create_read_only` once before
/// invoking this to bootstrap the schema.
///
/// The import runs inside a single SQLite transaction, so a parse
/// or constraint failure leaves the destination untouched.
pub fn import_json_backup(target_path: &Path, source_path: &Path) -> Result<ImportStats> {
    let source_path = normalize_windows_path(source_path);
    let source_path = source_path.as_path();
    // H-4: bound the input before touching it. `fs::metadata` is
    // cheap and gives us the size without opening for read. Rejecting
    // here means we never allocate a buffer for an adversarial file.
    // Use "read" in the user-facing prefix so missing-file
    // callers see a message they can parse — matches the wording
    // the old `read_to_string` error path used.
    let meta = std::fs::metadata(source_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("read {}: {e}", source_path.display()),
        ))
    })?;
    if meta.len() > MAX_BACKUP_BYTES {
        return Err(CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "backup {} is {} bytes; import cap is {} bytes",
                source_path.display(),
                meta.len(),
                MAX_BACKUP_BYTES,
            ),
        )));
    }
    // Stream-parse from a buffered reader instead of
    // `fs::read_to_string`. serde_json's reader API allocates
    // internally but avoids the extra whole-file-in-String copy,
    // and its default recursion limit (128) still guards against
    // deeply-nested JSON stack blow-up.
    let file = File::open(source_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("read {}: {e}", source_path.display()),
        ))
    })?;
    let reader = BufReader::new(file);
    let backup: BackupRoot = serde_json::from_reader(reader).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("parse {}: {e}", source_path.display()),
        ))
    })?;
    tracing::info!(
        "importing backup version={} timestamp={} from {}",
        backup.version,
        backup.timestamp,
        source_path.display(),
    );

    let mut conn = Connection::open(target_path)?;
    let tx = conn.transaction()?;
    let stats = apply(&tx, &backup.data)?;
    tx.commit()?;

    tracing::info!("import complete: {stats:?}");
    Ok(stats)
}

fn apply(tx: &rusqlite::Transaction<'_>, data: &BackupData) -> Result<ImportStats> {
    let mut stats = ImportStats::default();

    // Top-level entities first — they have no FKs back to tasks.
    for p in &data.places {
        tx.execute(
            "INSERT OR REPLACE INTO places \
             (uid, name, address, phone, url, latitude, longitude, \
              place_color, place_icon, place_order, radius) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                p.uid,
                p.name,
                p.address,
                p.phone,
                p.url,
                p.latitude,
                p.longitude,
                p.color,
                p.icon,
                p.order,
                p.radius,
            ],
        )?;
        stats.places += 1;
    }

    for td in &data.tags {
        tx.execute(
            "INSERT OR REPLACE INTO tagdata \
             (remoteId, name, color, tagOrdering, td_icon, td_order) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                td.remote_id,
                td.name,
                td.color,
                td.tag_ordering,
                td.icon,
                td.order,
            ],
        )?;
        stats.tag_data += 1;
    }

    for f in &data.filters {
        // C-1: do NOT persist the imported `sql` string verbatim.
        // The Android backup carries user-authored (or 3rd-party-authored)
        // SQL fragments the Android app later splices into its recursive
        // query; a hostile backup can hide, e.g.,
        //   `EXISTS (SELECT 1 FROM caldav_accounts WHERE tasks.title = cda_password)`
        // there, which on desktop surfaces CalDAV passwords as task titles
        // when the user clicks the saved filter. Write NULL so the
        // read path in `query::run_by_filter_id` returns an empty list
        // rather than executing an untrusted fragment. The structured
        // `criterion` column is preserved; a future desktop builder can
        // rebuild trusted SQL from it without touching this path.
        let _dropped_sql = f.sql.as_deref(); // surfaced only for the log.
        if _dropped_sql.is_some() {
            tracing::debug!(
                filter = f.title.as_deref().unwrap_or(""),
                "dropping imported `sql` fragment; custom filters from backups \
                 are no-ops until the desktop builds SQL from `criterion`",
            );
        }
        tx.execute(
            "INSERT OR REPLACE INTO filters \
             (title, sql, \"values\", criterion, f_color, f_icon, f_order) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                f.title,
                Option::<String>::None, // <- was `f.sql` — neutralised
                f.values,
                f.criterion,
                f.color,
                f.icon,
                f.order
            ],
        )?;
        stats.filters += 1;
    }

    for a in &data.caldav_accounts {
        tx.execute(
            "INSERT OR REPLACE INTO caldav_accounts \
             (cda_uuid, cda_name, cda_url, cda_username, cda_password, cda_error, \
              cda_account_type, cda_collapsed, cda_server_type, cda_last_sync) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                a.uuid,
                a.name,
                a.url,
                a.username,
                a.password,
                a.error,
                a.account_type,
                a.is_collapsed as i32,
                a.server_type,
                a.last_sync,
            ],
        )?;
        stats.caldav_accounts += 1;
    }

    for c in &data.caldav_calendars {
        tx.execute(
            "INSERT OR REPLACE INTO caldav_lists \
             (cdl_account, cdl_uuid, cdl_name, cdl_color, cdl_ctag, cdl_url, \
              cdl_icon, cdl_order, cdl_access, cdl_last_sync) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                c.account,
                c.uuid,
                c.name,
                c.color,
                c.ctag,
                c.url,
                c.icon,
                c.order,
                c.access,
                c.last_sync,
            ],
        )?;
        stats.caldav_calendars += 1;
    }

    // Tasks and their per-task children. `INSERT OR REPLACE` keyed
    // on the UNIQUE `remoteId` index makes re-importing the same
    // backup idempotent — Room's `@ForeignKey(onDelete = CASCADE)`
    // on alarms / tags / geofences / caldav_tasks wipes the stale
    // dependents before we re-insert them pointing at the new _id.
    //
    // task.parent stays 0; see module docs on the parent-
    // restoration limitation.
    for bundle in &data.tasks {
        let t = &bundle.task;
        tx.execute(
            "INSERT OR REPLACE INTO tasks \
             (title, importance, dueDate, hideUntil, created, modified, \
              completed, deleted, notes, estimatedSeconds, elapsedSeconds, \
              timerStart, notificationFlags, lastNotified, recurrence, \
              repeat_from, calendarUri, remoteId, collapsed, parent, \
              \"order\", read_only) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, \
                     ?13, ?14, ?15, ?16, ?17, ?18, ?19, 0, ?20, ?21)",
            params![
                t.title,
                t.priority,
                t.due_date,
                t.hide_until,
                t.creation_date,
                t.modification_date,
                t.completion_date,
                t.deletion_date,
                t.notes,
                t.estimated_seconds,
                t.elapsed_seconds,
                t.timer_start,
                t.ring_flags,
                t.reminder_last,
                t.recurrence,
                t.repeat_from,
                t.calendar_uri,
                t.remote_id,
                t.is_collapsed as i32,
                t.order,
                t.read_only as i32,
            ],
        )?;
        let new_task_id = tx.last_insert_rowid();
        stats.tasks += 1;

        for a in &bundle.alarms {
            tx.execute(
                "INSERT INTO alarms (task, time, type, repeat, interval) \
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![new_task_id, a.time, a.alarm_type, a.repeat, a.interval],
            )?;
            stats.alarms += 1;
        }

        for g in &bundle.geofences {
            tx.execute(
                "INSERT INTO geofences (task, place, arrival, departure) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![new_task_id, g.place, g.arrival as i32, g.departure as i32],
            )?;
            stats.geofences += 1;
        }

        for tag in &bundle.tags {
            tx.execute(
                "INSERT INTO tags (task, name, tag_uid, task_uid) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![new_task_id, tag.name, tag.tag_uid, t.remote_id],
            )?;
            stats.tags += 1;
        }

        for ct in &bundle.caldav_tasks {
            tx.execute(
                "INSERT INTO caldav_tasks \
                 (cd_task, cd_calendar, cd_remote_id, cd_object, cd_etag, \
                  cd_last_sync, cd_deleted, cd_remote_parent, gt_moved, gt_remote_order) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    new_task_id,
                    ct.calendar,
                    ct.remote_id,
                    ct.object_name,
                    ct.etag,
                    ct.last_sync,
                    ct.deleted,
                    ct.remote_parent,
                    ct.is_moved as i32,
                    ct.remote_order,
                ],
            )?;
            stats.caldav_tasks += 1;
        }

        stats.skipped_attachments += bundle.attachments.len();
        stats.skipped_comments += bundle.comments.len();
    }

    stats.subtask_links = relink_subtasks(tx)?;

    Ok(stats)
}

/// Walk the caldav-task graph and stamp `tasks.parent` for every
/// subtask whose parent is also in this import.
///
/// We key on `caldav_tasks.cd_remote_id` (the CalDAV object UID,
/// which the Android app uses as the stable cross-device identity
/// for a task): every child row contributes its own
/// `cd_remote_parent` pointing at its parent's `cd_remote_id`. The
/// local `tasks._id` isn't portable, so we resolve
/// remote-id → local-id via an in-memory HashMap built from
/// the newly-inserted caldav_tasks rows.
///
/// Notes:
/// - One CalDAV calendar can hold a whole subtask tree; we don't
///   scope the lookup to a single calendar because the backup format
///   already guarantees one CalDAV row per task per calendar.
/// - If a child's `cd_remote_parent` points at a UID not present in
///   this backup, we log at debug and leave `tasks.parent = 0`
///   (orphan); re-importing alongside the missing parent will pick
///   the link up on the second pass.
/// - A task may appear in multiple CalDAV rows (rare, but possible
///   if the same task is replicated across calendars). We key the
///   map on `cd_remote_id` and accept the last writer wins — all
///   values should point at the same `tasks._id` for a given UID
///   because `tasks.remoteId` is UNIQUE.
fn relink_subtasks(tx: &rusqlite::Transaction<'_>) -> Result<usize> {
    let mut remote_to_task_id: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();
    {
        let mut stmt = tx.prepare(
            "SELECT cd_task, cd_remote_id FROM caldav_tasks \
             WHERE cd_remote_id IS NOT NULL AND cd_remote_id != ''",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (task_id, remote_id) = row?;
            remote_to_task_id.insert(remote_id, task_id);
        }
    }

    // Drop the prepared statement explicitly before the map-prepared
    // UPDATE below runs — otherwise clippy's let_and_return fights
    // borrowck's drop order on the nested `query_map` temporary.
    let mut edge_stmt = tx.prepare(
        "SELECT cd_task, cd_remote_parent FROM caldav_tasks \
         WHERE cd_remote_parent IS NOT NULL AND cd_remote_parent != ''",
    )?;
    let edges: Vec<(i64, String)> = edge_stmt
        .query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(edge_stmt);

    let mut update_stmt = tx.prepare("UPDATE tasks SET parent = ?1 WHERE _id = ?2")?;
    let mut linked = 0;
    for (child_task_id, remote_parent) in edges {
        match remote_to_task_id.get(&remote_parent) {
            Some(&parent_task_id) if parent_task_id != child_task_id => {
                update_stmt.execute(params![parent_task_id, child_task_id])?;
                linked += 1;
            }
            Some(_) => {
                // Self-parent: the remote_parent UID resolved back to
                // the same task. Malformed; don't write a cycle.
                tracing::warn!(
                    "task {child_task_id} claims itself as remote parent ('{remote_parent}'); skipping"
                );
            }
            None => {
                tracing::debug!(
                    "orphan subtask: task {child_task_id} points at remote parent '{remote_parent}' with no match in this backup"
                );
            }
        }
    }
    Ok(linked)
}

#[cfg(test)]
mod path_tests {
    use super::normalize_windows_path;
    use std::path::Path;

    #[test]
    fn strips_leading_slash_before_drive_letter() {
        // Reported Windows regression: QML `file:///C:/...` stripped
        // naively gives `/C:/...`, which CreateFileW rejects with
        // OS error 123. normalizer must remove the leading slash.
        assert_eq!(
            normalize_windows_path(Path::new("/C:/Users/cshiflet/x.json")),
            Path::new("C:/Users/cshiflet/x.json")
        );
        assert_eq!(
            normalize_windows_path(Path::new("/z:/data")),
            Path::new("z:/data")
        );
    }

    #[test]
    fn leaves_real_unix_paths_alone() {
        assert_eq!(
            normalize_windows_path(Path::new("/home/user/x.json")),
            Path::new("/home/user/x.json")
        );
        assert_eq!(
            normalize_windows_path(Path::new("/Users/foo/Downloads/x.json")),
            Path::new("/Users/foo/Downloads/x.json")
        );
    }

    #[test]
    fn leaves_native_windows_paths_alone() {
        // Users who paste an already-native path via a CLI shouldn't
        // have it re-mangled.
        assert_eq!(
            normalize_windows_path(Path::new("C:/Users/cshiflet/x.json")),
            Path::new("C:/Users/cshiflet/x.json")
        );
    }
}
