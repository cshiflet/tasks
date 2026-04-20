//! JSON backup import.
//!
//! Reads the JSON format produced by
//! `app/src/main/java/org/tasks/backup/TasksJsonExporter.kt` and
//! inserts every entity into a target SQLite file. Used to seed a
//! fresh desktop database from an Android export.
//!
//! Known limitations (explicit, documented):
//!
//! * **Parent-child subtask links are not restored.** Kotlin marks
//!   `Task.parent: Long` as `@Transient`, so the JSON doesn't carry
//!   it. Subtasks appear as flat tasks after import. A future pass
//!   can re-link them by walking `caldavTasks.remoteParent` (a UUID
//!   pointing at the owning task's `remoteId`).
//! * **Attachments and user-activity comments are skipped.** The
//!   export carries their metadata but the file contents live
//!   elsewhere (URI references); restoring those faithfully is its
//!   own packaging concern.
//! * **Task list metadata and Astrid-era legacy locations** are
//!   skipped for the same reason — rarely populated on a modern
//!   install.
//!
//! Import is transactional: either every row lands or nothing
//! does. Existing rows at the same primary key are *replaced*
//! (`INSERT OR REPLACE`), so re-importing the same backup is
//! idempotent.

use std::path::Path;

use rusqlite::{params, Connection};
use serde::Deserialize;

use crate::error::{CoreError, Result};

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

/// Read a Tasks.org JSON backup from `source_path` and apply it to
/// the database at `target_path`. The target file must already
/// exist — call `Database::open_or_create_read_only` once before
/// invoking this to bootstrap the schema.
///
/// The import runs inside a single SQLite transaction, so a parse
/// or constraint failure leaves the destination untouched.
pub fn import_json_backup(target_path: &Path, source_path: &Path) -> Result<ImportStats> {
    let raw = std::fs::read_to_string(source_path).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("read {}: {e}", source_path.display()),
        ))
    })?;
    let backup: BackupRoot = serde_json::from_str(&raw).map_err(|e| {
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
        tx.execute(
            "INSERT OR REPLACE INTO filters \
             (title, sql, \"values\", criterion, f_color, f_icon, f_order) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                f.title,
                f.sql,
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

    Ok(stats)
}
