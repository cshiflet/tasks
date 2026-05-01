//! Microsoft Graph (To Do) payload shapes + translation.
//!
//! Microsoft To Do surfaces as the Graph API under
//! `/me/todo/lists` and `/me/todo/lists/{id}/tasks`. The shapes
//! here cover the subset the desktop uses to pull + push; the
//! HTTP layer (loopback OAuth → reqwest GET/POST/PATCH/DELETE)
//! plugs in on top.
//!
//! Docs: <https://learn.microsoft.com/graph/api/resources/todo-overview>

use serde::{Deserialize, Serialize};

use crate::provider::{RemoteCalendar, RemoteTask, SyncError, SyncResult};

/// `todoTaskList` resource.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct TodoTaskListJson {
    pub id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "isOwner")]
    #[serde(default)]
    pub is_owner: bool,
    #[serde(rename = "isShared")]
    #[serde(default)]
    pub is_shared: bool,
    #[serde(rename = "wellknownListName")]
    pub wellknown_list_name: Option<String>,
    #[serde(rename = "@odata.etag")]
    pub etag: Option<String>,
}

/// `dateTimeTimeZone` complex type — Graph's shape for any
/// date-time with an explicit timezone. We normalise to ms-since-
/// epoch UTC.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct DateTimeTimeZone {
    #[serde(rename = "dateTime")]
    pub date_time: String,
    #[serde(rename = "timeZone")]
    pub time_zone: String,
}

/// `todoTask` resource.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct TodoTaskJson {
    pub id: String,
    pub title: Option<String>,
    pub body: Option<TaskBody>,
    /// `"notStarted"` | `"inProgress"` | `"completed"` |
    /// `"waitingOnOthers"` | `"deferred"`.
    pub status: Option<String>,
    /// `"low"` | `"normal"` | `"high"`.
    pub importance: Option<String>,
    #[serde(rename = "dueDateTime")]
    pub due_date_time: Option<DateTimeTimeZone>,
    #[serde(rename = "completedDateTime")]
    pub completed_date_time: Option<DateTimeTimeZone>,
    #[serde(rename = "lastModifiedDateTime")]
    pub last_modified: Option<String>,
    #[serde(rename = "createdDateTime")]
    pub created: Option<String>,
    pub recurrence: Option<serde_json::Value>,
    #[serde(rename = "@odata.etag")]
    pub etag: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Default)]
pub struct TaskBody {
    pub content: Option<String>,
    /// `"text"` | `"html"`.
    #[serde(rename = "contentType")]
    pub content_type: Option<String>,
}

/// `@odata.nextLink`-style envelope for list responses.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct OdataList<T> {
    #[serde(default = "Vec::new", rename = "value")]
    pub value: Vec<T>,
    #[serde(rename = "@odata.nextLink")]
    pub next_link: Option<String>,
}

pub fn parse_task_lists(body: &str) -> SyncResult<OdataList<TodoTaskListJson>> {
    serde_json::from_str(body).map_err(|e| SyncError::Protocol(format!("todo lists JSON: {e}")))
}

pub fn parse_tasks(body: &str) -> SyncResult<OdataList<TodoTaskJson>> {
    serde_json::from_str(body).map_err(|e| SyncError::Protocol(format!("todo tasks JSON: {e}")))
}

pub fn task_list_to_remote_calendar(list: &TodoTaskListJson) -> RemoteCalendar {
    RemoteCalendar {
        remote_id: list.id.clone(),
        name: list.display_name.clone(),
        url: None,
        color: None,
        change_tag: list.etag.clone(),
        // shared lists we don't own are read-only.
        read_only: list.is_shared && !list.is_owner,
    }
}

pub fn task_to_remote(task: &TodoTaskJson, calendar_remote_id: &str) -> RemoteTask {
    let due_ms = task
        .due_date_time
        .as_ref()
        .and_then(|d| parse_graph_datetime(&d.date_time, &d.time_zone))
        .unwrap_or(0);
    let due_has_time = due_ms != 0 && due_ms % 86_400_000 != 0;

    let completed_ms = if let Some(c) = task.completed_date_time.as_ref() {
        parse_graph_datetime(&c.date_time, &c.time_zone).unwrap_or(0)
    } else if task
        .status
        .as_deref()
        .map(|s| s.eq_ignore_ascii_case("completed"))
        .unwrap_or(false)
    {
        // Status says completed but we don't have a stamp — use
        // last-modified as the fallback so the active-list
        // filter hides the row.
        task.last_modified
            .as_deref()
            .and_then(|s| parse_graph_datetime(s, "UTC"))
            .unwrap_or(1)
    } else {
        0
    };

    let priority = match task.importance.as_deref() {
        Some("high") => 0,
        Some("low") => 2,
        Some("normal") => 1,
        _ => 3,
    };

    RemoteTask {
        remote_id: task.id.clone(),
        calendar_remote_id: calendar_remote_id.to_string(),
        etag: task.etag.clone(),
        title: task.title.clone(),
        notes: task.body.as_ref().and_then(|b| b.content.clone()),
        due_ms,
        due_has_time,
        completed_ms,
        priority,
        // Graph carries recurrence as a nested object, not an
        // RRULE. Converting is a TODO; for now, tasks with
        // structured recurrence round-trip unchanged on the
        // local side because we never overwrite the stored
        // recurrence on pull.
        recurrence: None,
        // Graph doesn't expose a parent/subtask concept directly
        // at the API; linked tasks live under
        // /me/todo/lists/{id}/tasks/{id}/linkedResources.
        parent_remote_id: None,
        raw_vtodo: None,
    }
}

pub fn remote_to_task_json(task: &RemoteTask) -> serde_json::Value {
    let status = if task.completed_ms > 0 {
        "completed"
    } else {
        "notStarted"
    };
    let importance = match task.priority {
        0 => "high",
        1 => "normal",
        2 => "low",
        _ => "normal",
    };
    let mut obj = serde_json::Map::new();
    obj.insert("status".into(), serde_json::Value::String(status.into()));
    obj.insert(
        "importance".into(),
        serde_json::Value::String(importance.into()),
    );
    if let Some(title) = &task.title {
        obj.insert("title".into(), serde_json::Value::String(title.clone()));
    }
    if let Some(notes) = &task.notes {
        obj.insert(
            "body".into(),
            serde_json::json!({
                "content": notes,
                "contentType": "text",
            }),
        );
    }
    if task.due_ms > 0 {
        obj.insert(
            "dueDateTime".into(),
            serde_json::json!({
                "dateTime": graph_datetime_iso(task.due_ms),
                "timeZone": "UTC",
            }),
        );
    }
    if task.completed_ms > 0 {
        obj.insert(
            "completedDateTime".into(),
            serde_json::json!({
                "dateTime": graph_datetime_iso(task.completed_ms),
                "timeZone": "UTC",
            }),
        );
    }
    serde_json::Value::Object(obj)
}

/// Parse Graph's `dateTime` string in the given `timeZone`. Graph
/// uses Windows-style zone IDs (`"UTC"`, `"Pacific Standard Time"`)
/// for local times and ISO strings with `Z` elsewhere. We handle
/// the two common cases: UTC (trivial) and a local timestamp
/// shaped like `YYYY-MM-DDTHH:MM:SS(.ffffff)?` that we treat as
/// UTC for now (timezone-aware parsing is a follow-up).
pub fn parse_graph_datetime(value: &str, tz: &str) -> Option<i64> {
    let mut s = value.trim();
    // Strip fractional seconds if present.
    if let Some(dot) = s.find('.') {
        // Find the end of the fractional component (digits only).
        let tail_start = s[dot + 1..]
            .char_indices()
            .find(|(_, c)| !c.is_ascii_digit())
            .map(|(i, _)| dot + 1 + i)
            .unwrap_or(s.len());
        // Rebuild without the fractional part.
        let without = format!("{}{}", &s[..dot], &s[tail_start..]);
        // Store in a local so the &str borrow lives long enough.
        return parse_graph_datetime_stripped(&without, tz);
    }
    // Ensure we end with a tz marker for the RFC 3339 parser.
    let owned;
    if !s.ends_with('Z') && !s.contains('+') && s[11..].find('-').is_none() {
        owned = format!("{s}Z");
        s = owned.as_str();
        return super::super::providers::google_json::rfc3339_to_ms(s)
            .or_else(|| tz_unhandled(tz, value));
    }
    super::super::providers::google_json::rfc3339_to_ms(s).or_else(|| tz_unhandled(tz, value))
}

fn parse_graph_datetime_stripped(s: &str, tz: &str) -> Option<i64> {
    let owned;
    let s = if !s.ends_with('Z') && !s.contains('+') && s[11..].find('-').is_none() {
        owned = format!("{s}Z");
        owned.as_str()
    } else {
        s
    };
    super::super::providers::google_json::rfc3339_to_ms(s).or_else(|| tz_unhandled(tz, s))
}

/// A timezone we don't handle yet — log at debug and fall back
/// to None so the caller knows to keep the previous value.
fn tz_unhandled(tz: &str, raw: &str) -> Option<i64> {
    if !tz.eq_ignore_ascii_case("UTC") {
        tracing::debug!("unhandled Graph timezone {tz} on {raw}");
    }
    None
}

pub fn graph_datetime_iso(ms: i64) -> String {
    // Graph accepts dateTime without a Z suffix in the complex
    // type; the companion `timeZone: "UTC"` carries the offset.
    let secs = ms.div_euclid(1000);
    let day = secs.div_euclid(86_400);
    let time = secs - day * 86_400;
    let (y, m, d) = tasks_core::datetime::days_to_ymd(day);
    let h = time / 3600;
    let mi = (time % 3600) / 60;
    let s = time % 60;
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}.0000000")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_todo_lists() {
        let body = r#"{
          "value": [
            {
              "id": "AQMk",
              "displayName": "Tasks",
              "isOwner": true,
              "isShared": false,
              "wellknownListName": "defaultList"
            }
          ]
        }"#;
        let parsed = parse_task_lists(body).unwrap();
        assert_eq!(parsed.value.len(), 1);
        assert_eq!(parsed.value[0].display_name, "Tasks");
        assert!(parsed.value[0].is_owner);
    }

    #[test]
    fn task_maps_to_remote_basic() {
        let t = TodoTaskJson {
            id: "abc".into(),
            title: Some("Ship".into()),
            body: Some(TaskBody {
                content: Some("Ship the thing".into()),
                content_type: Some("text".into()),
            }),
            status: Some("notStarted".into()),
            importance: Some("high".into()),
            due_date_time: Some(DateTimeTimeZone {
                date_time: "2024-01-15T00:00:00".into(),
                time_zone: "UTC".into(),
            }),
            ..Default::default()
        };
        let rt = task_to_remote(&t, "cal-1");
        assert_eq!(rt.title.as_deref(), Some("Ship"));
        assert_eq!(rt.notes.as_deref(), Some("Ship the thing"));
        assert_eq!(rt.priority, 0); // high
        assert_eq!(rt.due_ms, 1_705_276_800_000);
        assert!(!rt.due_has_time);
    }

    #[test]
    fn shared_non_owner_list_is_read_only() {
        let list = TodoTaskListJson {
            id: "x".into(),
            display_name: "Shared".into(),
            is_owner: false,
            is_shared: true,
            wellknown_list_name: None,
            etag: None,
        };
        assert!(task_list_to_remote_calendar(&list).read_only);
    }

    #[test]
    fn remote_to_task_json_round_trip() {
        let rt = RemoteTask {
            remote_id: "abc".into(),
            calendar_remote_id: "cal-1".into(),
            etag: None,
            title: Some("T".into()),
            notes: Some("N".into()),
            due_ms: 1_705_341_600_000,
            due_has_time: true,
            completed_ms: 0,
            priority: 1,
            recurrence: None,
            parent_remote_id: None,
            raw_vtodo: None,
        };
        let v = remote_to_task_json(&rt);
        assert_eq!(v["status"], "notStarted");
        assert_eq!(v["importance"], "normal");
        assert_eq!(v["title"], "T");
        assert_eq!(v["body"]["content"], "N");
        assert_eq!(v["dueDateTime"]["timeZone"], "UTC");
    }

    #[test]
    fn importance_buckets() {
        for (graph, expected) in [
            ("high", 0_i32),
            ("normal", 1),
            ("low", 2),
            ("unknown-value", 3),
        ] {
            let t = TodoTaskJson {
                id: "x".into(),
                importance: Some(graph.into()),
                ..Default::default()
            };
            assert_eq!(task_to_remote(&t, "c").priority, expected, "{graph}");
        }
    }

    #[test]
    fn graph_datetime_strips_fractional_seconds() {
        // Graph often emits "2024-01-15T18:00:00.0000000"; we
        // drop the fraction and parse as UTC.
        assert_eq!(
            parse_graph_datetime("2024-01-15T18:00:00.0000000", "UTC"),
            Some(1_705_341_600_000)
        );
    }
}
