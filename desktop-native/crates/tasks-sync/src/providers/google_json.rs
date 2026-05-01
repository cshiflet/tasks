//! Google Tasks REST API payload shapes + translation into the
//! shared [`crate::RemoteTask`] / [`crate::RemoteCalendar`] types.
//!
//! The HTTP layer (coming later, once the loopback OAuth + reqwest
//! wiring lands) hands raw JSON response bodies to the parsers
//! here. No network, no I/O — every test is a fixture.
//!
//! Source of truth: <https://developers.google.com/tasks/reference/rest>.
//! The subset we care about:
//!
//! * `tasklists.list` returns `{ items: [TaskList …] }`.
//! * `tasks.list` returns `{ items: [Task …], nextPageToken? }`.
//! * `tasks.insert/update` takes a single Task JSON body.
//!
//! Dates are RFC 3339 strings (`2024-01-15T18:00:00.000Z`); we
//! parse them into ms-since-epoch for consistency with the rest
//! of the codebase.

use serde::{Deserialize, Serialize};

use crate::provider::{RemoteCalendar, RemoteTask, SyncError, SyncResult};

/// `tasks#taskList` resource.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct TaskListJson {
    pub id: String,
    pub title: String,
    pub updated: Option<String>,
    pub etag: Option<String>,
    #[serde(rename = "selfLink")]
    pub self_link: Option<String>,
}

/// `tasks#task` resource.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct TaskJson {
    pub id: String,
    pub title: Option<String>,
    pub notes: Option<String>,
    /// `"needsAction"` | `"completed"`.
    pub status: Option<String>,
    /// RFC 3339; Google Tasks only honours the date part (no
    /// time-of-day) per the API docs.
    pub due: Option<String>,
    pub completed: Option<String>,
    pub updated: Option<String>,
    pub parent: Option<String>,
    pub position: Option<String>,
    pub deleted: Option<bool>,
    pub hidden: Option<bool>,
    pub etag: Option<String>,
    #[serde(rename = "selfLink")]
    pub self_link: Option<String>,
}

/// Envelope for `tasklists.list` responses.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TaskListListResponse {
    #[serde(default)]
    pub items: Vec<TaskListJson>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

/// Envelope for `tasks.list` responses.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct TaskListResponse {
    #[serde(default)]
    pub items: Vec<TaskJson>,
    #[serde(rename = "nextPageToken")]
    pub next_page_token: Option<String>,
}

pub fn parse_tasklists(body: &str) -> SyncResult<TaskListListResponse> {
    serde_json::from_str(body).map_err(|e| SyncError::Protocol(format!("tasklists JSON: {e}")))
}

pub fn parse_tasks(body: &str) -> SyncResult<TaskListResponse> {
    serde_json::from_str(body).map_err(|e| SyncError::Protocol(format!("tasks JSON: {e}")))
}

pub fn tasklist_to_remote_calendar(list: &TaskListJson) -> RemoteCalendar {
    RemoteCalendar {
        remote_id: list.id.clone(),
        name: list.title.clone(),
        url: list.self_link.clone(),
        color: None, // Google Tasks doesn't expose per-list colour
        change_tag: list.etag.clone(),
        read_only: false,
    }
}

pub fn task_to_remote(task: &TaskJson, calendar_remote_id: &str) -> RemoteTask {
    let status = task.status.as_deref().unwrap_or("needsAction");
    let completed_ms = if let Some(completed) = task.completed.as_deref() {
        rfc3339_to_ms(completed).unwrap_or(0)
    } else if status.eq_ignore_ascii_case("completed") {
        // Server flagged completed but didn't stamp the time —
        // fall back to `updated` so the local query filter hides it.
        task.updated.as_deref().and_then(rfc3339_to_ms).unwrap_or(1)
    } else {
        0
    };

    let due_ms = task.due.as_deref().and_then(rfc3339_to_ms).unwrap_or(0);

    RemoteTask {
        remote_id: task.id.clone(),
        calendar_remote_id: calendar_remote_id.to_string(),
        etag: task.etag.clone(),
        title: task.title.clone(),
        notes: task.notes.clone(),
        due_ms,
        // Google Tasks' `due` is documented as date-only.
        due_has_time: false,
        completed_ms,
        // Google Tasks doesn't expose priority — everything is
        // NONE (3) in Tasks.org's bucket scheme.
        priority: 3,
        recurrence: None, // not supported by Google Tasks API
        parent_remote_id: task.parent.clone(),
        raw_vtodo: None,
    }
}

/// Build the JSON body for a Google Tasks insert/update. The UID
/// goes on the URL, not in the body; `id` is omitted so Google
/// accepts the call as either a create or a targeted update.
pub fn remote_to_task_json(task: &RemoteTask) -> serde_json::Value {
    let status = if task.completed_ms > 0 {
        "completed"
    } else {
        "needsAction"
    };
    let mut obj = serde_json::Map::new();
    obj.insert(
        "status".into(),
        serde_json::Value::String(status.to_string()),
    );
    if let Some(title) = &task.title {
        obj.insert("title".into(), serde_json::Value::String(title.clone()));
    }
    if let Some(notes) = &task.notes {
        obj.insert("notes".into(), serde_json::Value::String(notes.clone()));
    }
    if task.due_ms > 0 {
        obj.insert(
            "due".into(),
            serde_json::Value::String(ms_to_rfc3339(task.due_ms)),
        );
    }
    if task.completed_ms > 0 {
        obj.insert(
            "completed".into(),
            serde_json::Value::String(ms_to_rfc3339(task.completed_ms)),
        );
    }
    if let Some(parent) = &task.parent_remote_id {
        obj.insert("parent".into(), serde_json::Value::String(parent.clone()));
    }
    serde_json::Value::Object(obj)
}

// ---------- date helpers ----------

/// Parse an RFC 3339 timestamp into ms since epoch. Accepts
/// `YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)`. Returns None on any
/// shape we don't recognise so the caller can fall back to 0.
pub fn rfc3339_to_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    // Minimum: YYYY-MM-DDTHH:MM:SSZ
    if s.len() < 20 {
        return None;
    }
    let y: i32 = s.get(0..4)?.parse().ok()?;
    let m: u32 = s.get(5..7)?.parse().ok()?;
    let d: u32 = s.get(8..10)?.parse().ok()?;
    if &s[4..5] != "-" || &s[7..8] != "-" || &s[10..11] != "T" {
        return None;
    }
    let hh: i64 = s.get(11..13)?.parse().ok()?;
    let mm: i64 = s.get(14..16)?.parse().ok()?;
    let ss: i64 = s.get(17..19)?.parse().ok()?;
    if &s[13..14] != ":" || &s[16..17] != ":" {
        return None;
    }

    let days = tasks_core::datetime::ymd_to_days(y, m, d);
    let mut secs = days * 86_400 + hh * 3600 + mm * 60 + ss;

    // Handle timezone suffix.
    let rest = &s[19..];
    let (tz_index, offset_secs) = if rest.starts_with('Z') {
        (0, 0)
    } else if let Some(plus) = rest.find(['+', '-']) {
        let tz = &rest[plus..];
        if tz.len() < 6 {
            return None;
        }
        let sign: i64 = if &tz[..1] == "+" { 1 } else { -1 };
        let oh: i64 = tz.get(1..3)?.parse().ok()?;
        let om: i64 = tz.get(4..6)?.parse().ok()?;
        (plus, sign * (oh * 3600 + om * 60))
    } else {
        // No explicit offset → assume UTC.
        (rest.len(), 0)
    };
    // Fractional seconds (.123) — drop the fractional part, we
    // don't carry sub-second precision in tasks.dueDate.
    let _ = tz_index;

    secs -= offset_secs;
    Some(secs * 1000)
}

/// Format ms-since-epoch as RFC 3339 UTC
/// (`YYYY-MM-DDTHH:MM:SS.000Z`). Google accepts the Z form.
pub fn ms_to_rfc3339(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let day = secs.div_euclid(86_400);
    let time = secs - day * 86_400;
    let (y, m, d) = tasks_core::datetime::days_to_ymd(day);
    let h = time / 3600;
    let mi = (time % 3600) / 60;
    let s = time % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.000Z")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tasklists_envelope() {
        let body = r#"{
          "kind": "tasks#taskLists",
          "etag": "etag-outer",
          "items": [
            {"id": "MDE", "title": "Default", "updated": "2024-01-15T10:00:00.000Z"},
            {"id": "MDI", "title": "Work",    "updated": "2024-01-15T11:00:00.000Z"}
          ]
        }"#;
        let parsed = parse_tasklists(body).unwrap();
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.items[0].id, "MDE");
        assert_eq!(parsed.items[1].title, "Work");
    }

    #[test]
    fn parse_tasks_envelope_with_next_page_token() {
        let body = r#"{
          "kind": "tasks#tasks",
          "nextPageToken": "pg-2",
          "items": [
            {
              "id": "abc",
              "title": "Buy milk",
              "notes": "From the corner store",
              "status": "needsAction",
              "due": "2024-01-15T00:00:00.000Z",
              "updated": "2024-01-16T00:00:00.000Z",
              "parent": "def",
              "etag": "etag-abc"
            }
          ]
        }"#;
        let parsed = parse_tasks(body).unwrap();
        assert_eq!(parsed.next_page_token.as_deref(), Some("pg-2"));
        assert_eq!(parsed.items.len(), 1);
        let t = &parsed.items[0];
        assert_eq!(t.id, "abc");
        assert_eq!(t.title.as_deref(), Some("Buy milk"));
        assert_eq!(t.parent.as_deref(), Some("def"));
    }

    #[test]
    fn tasklist_maps_to_remote_calendar() {
        let tl = TaskListJson {
            id: "MDE".into(),
            title: "Default".into(),
            updated: None,
            etag: Some("et".into()),
            self_link: Some("https://…/MDE".into()),
        };
        let cal = tasklist_to_remote_calendar(&tl);
        assert_eq!(cal.remote_id, "MDE");
        assert_eq!(cal.name, "Default");
        assert_eq!(cal.change_tag.as_deref(), Some("et"));
        assert!(!cal.read_only);
    }

    #[test]
    fn task_maps_to_remote_with_due_and_completed() {
        let t = TaskJson {
            id: "abc".into(),
            title: Some("Ship".into()),
            notes: None,
            status: Some("completed".into()),
            due: Some("2024-01-15T00:00:00.000Z".into()),
            completed: Some("2024-01-17T12:34:56.000Z".into()),
            updated: None,
            parent: Some("root".into()),
            position: None,
            deleted: None,
            hidden: None,
            etag: Some("etag-1".into()),
            self_link: None,
        };
        let rt = task_to_remote(&t, "cal-1");
        assert_eq!(rt.title.as_deref(), Some("Ship"));
        assert_eq!(rt.due_ms, 1_705_276_800_000);
        assert!(!rt.due_has_time);
        // Jan 17 2024 12:34:56 UTC.
        assert_eq!(rt.completed_ms, 1_705_494_896_000);
        assert_eq!(rt.parent_remote_id.as_deref(), Some("root"));
        assert_eq!(rt.etag.as_deref(), Some("etag-1"));
    }

    #[test]
    fn task_completed_without_stamp_falls_back_to_updated() {
        let t = TaskJson {
            id: "x".into(),
            status: Some("completed".into()),
            updated: Some("2024-01-17T12:00:00.000Z".into()),
            ..Default::default()
        };
        let rt = task_to_remote(&t, "cal");
        assert_eq!(rt.completed_ms, 1_705_492_800_000);
    }

    #[test]
    fn remote_to_task_json_emits_expected_shape() {
        let rt = RemoteTask {
            remote_id: "abc".into(),
            calendar_remote_id: "cal-1".into(),
            etag: None,
            title: Some("Buy milk".into()),
            notes: Some("From the store".into()),
            due_ms: 1_705_276_800_000,
            due_has_time: false,
            completed_ms: 0,
            priority: 3,
            recurrence: None,
            parent_remote_id: Some("root".into()),
            raw_vtodo: None,
        };
        let v = remote_to_task_json(&rt);
        assert_eq!(v["status"], "needsAction");
        assert_eq!(v["title"], "Buy milk");
        assert_eq!(v["notes"], "From the store");
        assert_eq!(v["parent"], "root");
        assert_eq!(v["due"], "2024-01-15T00:00:00.000Z");
        assert!(v.get("completed").is_none());
    }

    #[test]
    fn rfc3339_round_trip() {
        let ms = 1_705_341_600_000_i64; // 2024-01-15T18:00:00Z
        let s = ms_to_rfc3339(ms);
        assert_eq!(s, "2024-01-15T18:00:00.000Z");
        assert_eq!(rfc3339_to_ms(&s), Some(ms));
    }

    #[test]
    fn rfc3339_handles_explicit_offset() {
        // 2024-01-15T10:00:00-08:00  → 2024-01-15T18:00:00Z
        assert_eq!(
            rfc3339_to_ms("2024-01-15T10:00:00-08:00"),
            Some(1_705_341_600_000)
        );
        // +05:30 (India)
        assert_eq!(
            rfc3339_to_ms("2024-01-15T23:30:00+05:30"),
            Some(1_705_341_600_000)
        );
    }

    #[test]
    fn rfc3339_rejects_obviously_bad_input() {
        assert_eq!(rfc3339_to_ms(""), None);
        assert_eq!(rfc3339_to_ms("not-a-date"), None);
        // Missing the T separator.
        assert_eq!(rfc3339_to_ms("2024-01-15 10:00:00Z"), None);
    }
}
