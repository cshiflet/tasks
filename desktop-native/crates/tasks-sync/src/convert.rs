//! Translate between [`crate::ical::VTodo`] (RFC 5545 wire shape)
//! and [`crate::RemoteTask`] (the normalised value type the
//! `Provider` trait speaks).
//!
//! Lossless for every field both sides represent. RRULE round-trips
//! verbatim. iCal's 0–9 priority maps onto Tasks.org's HIGH/MEDIUM/
//! LOW/NONE bucket per RFC 5545:
//! * 0       → NONE  (3) — "no priority assigned"
//! * 1–4     → HIGH  (0)
//! * 5       → MEDIUM (1)
//! * 6–9     → LOW   (2)
//!
//! Mapping back picks the canonical iCal value for each bucket
//! (HIGH=1, MEDIUM=5, LOW=9, NONE=0) so a HIGH→1→HIGH round trip is
//! stable, even though several iCal inputs would have collapsed
//! into the same bucket.
//!
//! Status reflects the COMPLETED stamp:
//! * `completed_ms > 0` → STATUS:COMPLETED
//! * otherwise → whatever was set on the wire (defaulting to
//!   NEEDS-ACTION when absent).

use crate::ical::{VTodo, VTodoStatus};
use crate::RemoteTask;

/// Maps Tasks.org's bucket priority (HIGH=0..NONE=3) onto a
/// canonical iCal priority value.
fn priority_to_ical(p: i32) -> u8 {
    match p {
        0 => 1, // HIGH
        1 => 5, // MEDIUM
        2 => 9, // LOW
        _ => 0, // NONE / unknown
    }
}

/// Maps an iCal 0–9 priority into the Tasks.org bucket value.
fn priority_from_ical(p: u8) -> i32 {
    match p {
        0 => 3,     // NONE
        1..=4 => 0, // HIGH
        5 => 1,     // MEDIUM
        _ => 2,     // LOW (6..=9 and any out-of-range value)
    }
}

/// Convert a parsed VTODO into a [`RemoteTask`] for the given
/// calendar. `raw_vtodo` is the original wire string when known
/// (so partial-update PUTs can preserve fields the desktop can't
/// edit).
pub fn vtodo_to_remote_task(
    vtodo: &VTodo,
    calendar_remote_id: &str,
    raw_vtodo: Option<String>,
) -> RemoteTask {
    let priority = vtodo.priority.map(priority_from_ical).unwrap_or(3);
    let parent = vtodo.parent_uid.clone();
    let completed_ms = match (vtodo.status, vtodo.completed_ms) {
        // Explicit COMPLETED:date wins.
        (_, Some(c)) => c,
        // STATUS:COMPLETED with no timestamp — pick the wire's
        // last-modified, or fall back to 1 (any non-zero) so the
        // active-list query hides it. Real Android emits both;
        // this is a defensive branch.
        (Some(VTodoStatus::Completed), None) => vtodo.last_modified_ms.unwrap_or(1),
        _ => 0,
    };
    RemoteTask {
        remote_id: vtodo.uid.clone(),
        calendar_remote_id: calendar_remote_id.to_string(),
        etag: None, // populated by the HTTP layer from the response header
        title: vtodo.summary.clone(),
        notes: vtodo.description.clone(),
        due_ms: vtodo.due_ms.unwrap_or(0),
        due_has_time: vtodo.due_has_time,
        completed_ms,
        priority,
        recurrence: vtodo.rrule.clone(),
        parent_remote_id: parent,
        raw_vtodo,
    }
}

/// Inverse of [`vtodo_to_remote_task`]. Rebuilds a VTODO suitable
/// for serialisation. Fields the local row didn't track (alarms,
/// categories, original CREATED stamp) come from `merge_into` if
/// passed — that lets a partial-update PUT preserve VALARM blocks
/// and CATEGORIES the desktop dialog doesn't yet edit. When
/// `merge_into` is `None`, we synthesise a fresh VTODO.
pub fn remote_task_to_vtodo(task: &RemoteTask, now_ms: i64, merge_into: Option<VTodo>) -> VTodo {
    let mut v = merge_into.unwrap_or_default();
    v.uid = task.remote_id.clone();
    v.summary = task.title.clone();
    v.description = task.notes.clone();
    v.priority = Some(priority_to_ical(task.priority));
    if task.due_ms > 0 {
        v.due_ms = Some(task.due_ms);
        v.due_has_time = task.due_has_time;
    } else {
        v.due_ms = None;
        v.due_has_time = false;
    }
    v.completed_ms = if task.completed_ms > 0 {
        Some(task.completed_ms)
    } else {
        None
    };
    v.status = Some(if task.completed_ms > 0 {
        VTodoStatus::Completed
    } else {
        VTodoStatus::NeedsAction
    });
    v.rrule = task.recurrence.clone();
    v.parent_uid = task.parent_remote_id.clone();
    v.last_modified_ms = Some(now_ms);
    if v.created_ms.is_none() {
        v.created_ms = Some(now_ms);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ical::{parse_vcalendar, serialize_vcalendar};

    #[test]
    fn priority_buckets_round_trip() {
        // HIGH bucket: any 1..=4 maps to HIGH (0); back to canonical 1.
        for input in [1, 2, 3, 4_u8] {
            assert_eq!(
                priority_to_ical(priority_from_ical(input)),
                1,
                "input {input}"
            );
        }
        // MEDIUM: only 5; back to 5.
        assert_eq!(priority_to_ical(priority_from_ical(5)), 5);
        // LOW: 6..=9; back to 9.
        for input in [6, 7, 8, 9_u8] {
            assert_eq!(
                priority_to_ical(priority_from_ical(input)),
                9,
                "input {input}"
            );
        }
        // NONE / undefined.
        assert_eq!(priority_to_ical(priority_from_ical(0)), 0);
    }

    #[test]
    fn vtodo_to_remote_and_back_preserves_payload() {
        let raw = "BEGIN:VCALENDAR\r\n\
BEGIN:VTODO\r\n\
UID:abc\r\n\
SUMMARY:Hello\r\n\
DESCRIPTION:Body\r\n\
PRIORITY:1\r\n\
DUE:20240115T180000Z\r\n\
STATUS:NEEDS-ACTION\r\n\
RRULE:FREQ=DAILY\r\n\
RELATED-TO;RELTYPE=PARENT:p\r\n\
END:VTODO\r\n\
END:VCALENDAR\r\n";
        let vtodo = parse_vcalendar(raw).unwrap();
        let task = vtodo_to_remote_task(&vtodo, "cal-1", Some(raw.to_string()));
        assert_eq!(task.title.as_deref(), Some("Hello"));
        assert_eq!(task.notes.as_deref(), Some("Body"));
        assert_eq!(task.priority, 0); // HIGH
        assert_eq!(task.due_ms, 1_705_341_600_000);
        assert_eq!(task.recurrence.as_deref(), Some("FREQ=DAILY"));
        assert_eq!(task.parent_remote_id.as_deref(), Some("p"));
        assert_eq!(task.completed_ms, 0);

        // Round-trip back: serialise and re-parse → same wire shape
        // for every field we round-trip. (Created/Last-Modified are
        // stamped by the conversion, so their values shift; we
        // only check the user-payload fields.)
        let rebuilt = remote_task_to_vtodo(&task, 1_700_000_000_000, Some(vtodo.clone()));
        let serialised = serialize_vcalendar(&rebuilt);
        let reparsed = parse_vcalendar(&serialised).unwrap();
        assert_eq!(reparsed.uid, vtodo.uid);
        assert_eq!(reparsed.summary, vtodo.summary);
        assert_eq!(reparsed.description, vtodo.description);
        assert_eq!(reparsed.due_ms, vtodo.due_ms);
        assert_eq!(reparsed.rrule, vtodo.rrule);
        assert_eq!(reparsed.parent_uid, vtodo.parent_uid);
    }

    #[test]
    fn completed_status_drives_completed_ms() {
        // When STATUS=COMPLETED but no COMPLETED stamp, fall back
        // to LAST-MODIFIED so the active-list filter hides the row.
        let raw = "BEGIN:VCALENDAR\r\n\
BEGIN:VTODO\r\n\
UID:x\r\n\
STATUS:COMPLETED\r\n\
LAST-MODIFIED:20240116T120000Z\r\n\
END:VTODO\r\n\
END:VCALENDAR\r\n";
        let vtodo = parse_vcalendar(raw).unwrap();
        let task = vtodo_to_remote_task(&vtodo, "cal", None);
        assert_eq!(task.completed_ms, 1_705_406_400_000);
    }
}
