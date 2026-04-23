//! Minimal iCalendar VTODO parser + serializer.
//!
//! Translates between the wire format CalDAV servers (and EteSync
//! collections) hand around and the [`crate::RemoteTask`] shape the
//! sync engine consumes. Pure Rust, zero I/O; every test runs from
//! string fixtures.
//!
//! **Scope:** the subset of RFC 5545 that Tasks.org actually emits
//! and consumes —
//!
//! * Top-level `VCALENDAR` envelope; we look at the first `VTODO`.
//! * VTODO properties: `UID`, `SUMMARY`, `DESCRIPTION`, `PRIORITY`,
//!   `DTSTART`, `DUE`, `COMPLETED`, `STATUS`, `RRULE`, `CATEGORIES`,
//!   `RELATED-TO;RELTYPE=PARENT`, `LAST-MODIFIED`, `CREATED`.
//! * Nested `VALARM` blocks with `ACTION` + `TRIGGER` (offset or
//!   absolute).
//!
//! **Out of scope (for now):**
//! * `VTIMEZONE` / `TZID` — every parsed datetime is treated as
//!   floating UTC. Tasks.org's writer also emits UTC for due
//!   dates, so this is consistent with the round-trip target.
//! * `RECURRENCE-ID`, `EXDATE`, `RDATE`.
//! * Attachments, comments, organiser, attendee.
//! * Multiple `VTODO`s in one `VCALENDAR` (a pull-cycle helper
//!   would iterate; we expose only the first.)

use std::fmt::Write as _;

use thiserror::Error;

use tasks_core::datetime::{days_to_ymd, ymd_to_days};

#[derive(Debug, Error)]
pub enum IcalError {
    #[error("missing required property: {0}")]
    MissingRequired(&'static str),
    #[error("malformed line: {0}")]
    MalformedLine(String),
    #[error("unsupported date/time value: {0}")]
    BadDate(String),
    #[error("no VTODO found in input")]
    NoVTodo,
    /// Input exceeded the 1 MiB parser cap. Real VCALENDAR bodies
    /// sit well under 100 KiB; anything larger is either wildly
    /// malformed or an attempt to exhaust memory via the parser.
    #[error("VCALENDAR input too large: {bytes} bytes")]
    TooLarge { bytes: usize },
    /// `VALARM`, `CATEGORIES`, or another bounded collection
    /// overflowed its per-VTODO cap. We stop parsing and surface
    /// the field name so callers can report which clip fired.
    #[error("too many {field} entries in VTODO (cap exceeded)")]
    TooMany { field: &'static str },
    /// Multiple `BEGIN:VTODO` blocks inside one VCALENDAR envelope.
    /// We used to silently return just the first, which could mask a
    /// CalDAV server packing two tasks into one response (L-1). Now
    /// we fail loudly so the caller surfaces a meaningful error
    /// rather than hiding data.
    #[error("VCALENDAR contained multiple VTODO blocks")]
    MultipleVtodos,
}

pub type IcalResult<T> = Result<T, IcalError>;

/// Hard cap on VCALENDAR input (M-3). Real bodies sit well under
/// 100 KiB; a multi-megabyte blob is a misbehaving server or an
/// attempt to exhaust memory. 1 MiB leaves plenty of headroom for
/// the occasional VCALENDAR with long CATEGORIES or a couple of
/// inline VALARMs.
pub const MAX_INPUT_BYTES: usize = 1024 * 1024;

/// Per-VTODO cap on VALARM blocks (M-3). Tasks.org's UI allows a
/// small handful; 128 is generous enough to survive round-tripping
/// a pathological calendar without letting a malicious payload
/// unbounded-allocate a `Vec<VAlarm>`.
pub const MAX_ALARMS_PER_VTODO: usize = 128;

/// Per-VTODO cap on CATEGORIES entries (M-3). 64 tags is more
/// than any reasonable user would attach; anything larger is
/// almost certainly adversarial.
pub const MAX_CATEGORIES_PER_VTODO: usize = 64;

/// One VTODO row, normalised. Mostly the wire-level representation
/// — translation onto [`crate::RemoteTask`] lives in
/// `crate::ical::convert`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VTodo {
    pub uid: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    /// Raw RFC 5545 priority (0–9). 0 = undefined, 1–4 high, 5
    /// medium, 6–9 low.
    pub priority: Option<u8>,
    pub dtstart_ms: Option<i64>,
    pub dtstart_has_time: bool,
    pub due_ms: Option<i64>,
    pub due_has_time: bool,
    pub completed_ms: Option<i64>,
    pub status: Option<VTodoStatus>,
    /// Raw RRULE (e.g. "FREQ=WEEKLY;BYDAY=MO,WE,FR").
    pub rrule: Option<String>,
    pub categories: Vec<String>,
    pub parent_uid: Option<String>,
    pub last_modified_ms: Option<i64>,
    pub created_ms: Option<i64>,
    pub alarms: Vec<VAlarm>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VTodoStatus {
    NeedsAction,
    InProcess,
    Completed,
    Cancelled,
}

impl VTodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VTodoStatus::NeedsAction => "NEEDS-ACTION",
            VTodoStatus::InProcess => "IN-PROCESS",
            VTodoStatus::Completed => "COMPLETED",
            VTodoStatus::Cancelled => "CANCELLED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VAlarm {
    pub action: String,
    pub trigger: AlarmTrigger,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlarmTrigger {
    /// Duration offset relative to DTSTART or DUE, in ms. Negative
    /// = before. RFC 5545 default is RELATED=START; Tasks.org emits
    /// END (i.e. offset from DUE), but we don't preserve the
    /// distinction yet — the desktop's edit dialog only edits
    /// before-due, so we always interpret as REL_END on round-trip.
    Offset(i64),
    /// Absolute UTC ms.
    Absolute(i64),
}

/// Parse a full `VCALENDAR` blob and return the first `VTODO` in it.
/// Calling code that needs every VTODO can extend this to return
/// a `Vec<VTodo>` — for the desktop's pull-cycle, one-per-call is
/// the natural shape (CalDAV reports usually wrap each VTODO in
/// its own VCALENDAR envelope).
pub fn parse_vcalendar(input: &str) -> IcalResult<VTodo> {
    // M-3 input cap: refuse to unfold or allocate for pathological
    // payloads. 1 MiB is far above any legitimate VCALENDAR size.
    if input.len() > MAX_INPUT_BYTES {
        return Err(IcalError::TooLarge { bytes: input.len() });
    }
    let lines = unfold(input);
    let mut depth: Vec<&str> = Vec::new();
    let mut current_todo: Option<VTodo> = None;
    let mut current_alarm: Option<VAlarm> = None;
    // L-1: hold onto the finished VTODO and reject a second
    // BEGIN:VTODO rather than silently dropping it.
    let mut completed_todo: Option<VTodo> = None;

    for raw_line in lines {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let parsed = parse_property(line)?;
        match (parsed.name.as_str(), parsed.value.as_str()) {
            ("BEGIN", "VCALENDAR") => depth.push("VCALENDAR"),
            ("BEGIN", "VTODO") => {
                // L-1: if we've already closed a VTODO, a second
                // BEGIN:VTODO means the envelope is packing multiple
                // todos — surface as MultipleVtodos.
                if completed_todo.is_some() {
                    return Err(IcalError::MultipleVtodos);
                }
                depth.push("VTODO");
                current_todo = Some(VTodo::default());
            }
            ("BEGIN", "VALARM") => {
                depth.push("VALARM");
                current_alarm = Some(VAlarm {
                    action: "DISPLAY".to_string(),
                    trigger: AlarmTrigger::Offset(0),
                });
            }
            ("END", "VALARM") => {
                if let (Some(todo), Some(alarm)) = (current_todo.as_mut(), current_alarm.take()) {
                    // M-3 alarm cap: fail loudly if a VTODO carries
                    // more than MAX_ALARMS_PER_VTODO VALARM blocks.
                    if todo.alarms.len() >= MAX_ALARMS_PER_VTODO {
                        return Err(IcalError::TooMany { field: "VALARM" });
                    }
                    todo.alarms.push(alarm);
                }
                depth.pop();
            }
            ("END", "VTODO") => {
                // Stash it for return; but keep walking so a second
                // BEGIN:VTODO inside the same envelope surfaces as
                // MultipleVtodos (L-1).
                if let Some(todo) = current_todo.take() {
                    completed_todo = Some(todo);
                }
                depth.pop();
            }
            ("END", "VCALENDAR") => {
                depth.pop();
            }
            _ => {
                if depth.last().copied() == Some("VALARM") {
                    if let Some(alarm) = current_alarm.as_mut() {
                        apply_valarm_property(alarm, &parsed)?;
                    }
                } else if depth.last().copied() == Some("VTODO") {
                    if let Some(todo) = current_todo.as_mut() {
                        apply_vtodo_property(todo, &parsed)?;
                    }
                }
                // Top-level VCALENDAR properties (PRODID, VERSION,
                // CALSCALE) are silently ignored — we don't need
                // them for the round-trip.
            }
        }
    }

    completed_todo.ok_or(IcalError::NoVTodo)
}

/// Serialize a VTODO into a complete VCALENDAR blob with our
/// PRODID. CRLF-terminated lines, folded at 75 octets.
pub fn serialize_vcalendar(todo: &VTodo) -> String {
    let mut out = String::new();
    write_line(&mut out, "BEGIN:VCALENDAR");
    write_line(&mut out, "VERSION:2.0");
    write_line(&mut out, "PRODID:-//tasks.org//tasks-desktop//EN");
    write_line(&mut out, "CALSCALE:GREGORIAN");
    write_line(&mut out, "BEGIN:VTODO");
    write_line(&mut out, &format!("UID:{}", escape_text(&todo.uid)));
    if let Some(s) = &todo.summary {
        write_line(&mut out, &format!("SUMMARY:{}", escape_text(s)));
    }
    if let Some(d) = &todo.description {
        write_line(&mut out, &format!("DESCRIPTION:{}", escape_text(d)));
    }
    if let Some(p) = todo.priority {
        write_line(&mut out, &format!("PRIORITY:{p}"));
    }
    if let Some(start_ms) = todo.dtstart_ms {
        write_line(
            &mut out,
            &format_date_property("DTSTART", start_ms, todo.dtstart_has_time),
        );
    }
    if let Some(due_ms) = todo.due_ms {
        write_line(
            &mut out,
            &format_date_property("DUE", due_ms, todo.due_has_time),
        );
    }
    if let Some(c) = todo.completed_ms {
        // COMPLETED is always a UTC datetime per RFC 5545.
        write_line(&mut out, &format!("COMPLETED:{}", format_utc_datetime(c)));
    }
    if let Some(status) = todo.status {
        write_line(&mut out, &format!("STATUS:{}", status.as_str()));
    }
    if let Some(rrule) = &todo.rrule {
        write_line(&mut out, &format!("RRULE:{rrule}"));
    }
    if !todo.categories.is_empty() {
        let joined: Vec<String> = todo.categories.iter().map(|c| escape_text(c)).collect();
        write_line(&mut out, &format!("CATEGORIES:{}", joined.join(",")));
    }
    if let Some(parent) = &todo.parent_uid {
        write_line(
            &mut out,
            &format!("RELATED-TO;RELTYPE=PARENT:{}", escape_text(parent)),
        );
    }
    if let Some(c) = todo.created_ms {
        write_line(&mut out, &format!("CREATED:{}", format_utc_datetime(c)));
    }
    if let Some(m) = todo.last_modified_ms {
        write_line(
            &mut out,
            &format!("LAST-MODIFIED:{}", format_utc_datetime(m)),
        );
    }
    for alarm in &todo.alarms {
        write_line(&mut out, "BEGIN:VALARM");
        write_line(&mut out, &format!("ACTION:{}", alarm.action));
        match alarm.trigger {
            AlarmTrigger::Offset(ms) => {
                write_line(&mut out, &format!("TRIGGER:{}", format_duration(ms)));
            }
            AlarmTrigger::Absolute(ms) => {
                write_line(
                    &mut out,
                    &format!("TRIGGER;VALUE=DATE-TIME:{}", format_utc_datetime(ms)),
                );
            }
        }
        write_line(&mut out, "END:VALARM");
    }
    write_line(&mut out, "END:VTODO");
    write_line(&mut out, "END:VCALENDAR");
    out
}

// ---------- internals ----------

#[derive(Debug)]
struct Property {
    name: String,
    params: Vec<(String, String)>,
    value: String,
}

fn parse_property(line: &str) -> IcalResult<Property> {
    // Split at first unquoted colon.
    let colon_pos = line
        .char_indices()
        .find(|(_, c)| *c == ':')
        .map(|(i, _)| i)
        .ok_or_else(|| IcalError::MalformedLine(line.to_string()))?;
    let header = &line[..colon_pos];
    let value = unescape_text(&line[colon_pos + 1..]);

    // L-2: a CR/LF inside an unescaped value byte is a protocol
    // smell; the unfolder should have stripped them. If one slips
    // through (e.g. an unescaped \r that the continuation rule
    // didn't match), reject outright to keep the downstream
    // serializer / HTTP-header handoff safe.
    if value.contains('\r') || value.contains('\n') {
        return Err(IcalError::MalformedLine(line.to_string()));
    }

    let mut header_parts = header.split(';');
    let raw_name = header_parts
        .next()
        .ok_or_else(|| IcalError::MalformedLine(line.to_string()))?;
    // L-2: empty property name (line starting with `:` or `;`) is
    // malformed; tolerate_and_drop was the previous behaviour,
    // which quietly masked bad data.
    if raw_name.trim().is_empty() {
        return Err(IcalError::MalformedLine(line.to_string()));
    }
    let name = raw_name.to_uppercase();
    let mut params = Vec::new();
    for part in header_parts {
        // L-2: a parameter without `=` (e.g. `UID;FOO:val`) is
        // malformed per RFC 5545 §3.2; reject rather than drop.
        let eq = part
            .find('=')
            .ok_or_else(|| IcalError::MalformedLine(line.to_string()))?;
        let pname = part[..eq].trim();
        if pname.is_empty() {
            return Err(IcalError::MalformedLine(line.to_string()));
        }
        let pname = pname.to_uppercase();
        let pval = part[eq + 1..].trim_matches('"').to_string();
        params.push((pname, pval));
    }
    Ok(Property {
        name,
        params,
        value,
    })
}

fn apply_vtodo_property(todo: &mut VTodo, p: &Property) -> IcalResult<()> {
    match p.name.as_str() {
        "UID" => todo.uid = p.value.clone(),
        "SUMMARY" => todo.summary = Some(p.value.clone()),
        "DESCRIPTION" => todo.description = Some(p.value.clone()),
        "PRIORITY" => {
            if let Ok(n) = p.value.trim().parse::<u8>() {
                if n <= 9 {
                    todo.priority = Some(n);
                }
            }
        }
        "DTSTART" => {
            let (ms, has_time) = parse_date_value(&p.value, p.params_value_kind())?;
            todo.dtstart_ms = Some(ms);
            todo.dtstart_has_time = has_time;
        }
        "DUE" => {
            let (ms, has_time) = parse_date_value(&p.value, p.params_value_kind())?;
            todo.due_ms = Some(ms);
            todo.due_has_time = has_time;
        }
        "COMPLETED" => {
            let (ms, _) = parse_date_value(&p.value, ParamValueKind::DateTime)?;
            todo.completed_ms = Some(ms);
        }
        "STATUS" => {
            todo.status = match p.value.to_uppercase().as_str() {
                "NEEDS-ACTION" => Some(VTodoStatus::NeedsAction),
                "IN-PROCESS" => Some(VTodoStatus::InProcess),
                "COMPLETED" => Some(VTodoStatus::Completed),
                "CANCELLED" => Some(VTodoStatus::Cancelled),
                _ => None,
            };
        }
        "RRULE" => todo.rrule = Some(p.value.clone()),
        "CATEGORIES" => {
            // M-3 categories cap. CATEGORIES can repeat across
            // multiple property lines (callers append), so count
            // the running total after extending.
            let mut extended = std::mem::take(&mut todo.categories);
            for piece in p.value.split(',') {
                let s = piece.trim();
                if s.is_empty() {
                    continue;
                }
                if extended.len() >= MAX_CATEGORIES_PER_VTODO {
                    return Err(IcalError::TooMany {
                        field: "CATEGORIES",
                    });
                }
                extended.push(s.to_string());
            }
            todo.categories = extended;
        }
        "RELATED-TO" => {
            // Default RELTYPE is PARENT per RFC 5545 §3.2.15.
            let reltype = p
                .params
                .iter()
                .find(|(n, _)| n == "RELTYPE")
                .map(|(_, v)| v.to_uppercase())
                .unwrap_or_else(|| "PARENT".to_string());
            if reltype == "PARENT" {
                todo.parent_uid = Some(p.value.clone());
            }
        }
        "LAST-MODIFIED" => {
            let (ms, _) = parse_date_value(&p.value, ParamValueKind::DateTime)?;
            todo.last_modified_ms = Some(ms);
        }
        "CREATED" => {
            let (ms, _) = parse_date_value(&p.value, ParamValueKind::DateTime)?;
            todo.created_ms = Some(ms);
        }
        _ => {} // Unknown property; ignore (X-vendor extensions, etc.)
    }
    Ok(())
}

fn apply_valarm_property(alarm: &mut VAlarm, p: &Property) -> IcalResult<()> {
    match p.name.as_str() {
        "ACTION" => alarm.action = p.value.clone(),
        "TRIGGER" => {
            let value_kind = p.params_value_kind();
            alarm.trigger = if value_kind == ParamValueKind::DateTime {
                let (ms, _) = parse_date_value(&p.value, ParamValueKind::DateTime)?;
                AlarmTrigger::Absolute(ms)
            } else {
                AlarmTrigger::Offset(parse_duration(&p.value)?)
            };
        }
        _ => {}
    }
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ParamValueKind {
    Default,
    Date,
    DateTime,
}

impl Property {
    fn params_value_kind(&self) -> ParamValueKind {
        for (name, value) in &self.params {
            if name == "VALUE" {
                return match value.to_uppercase().as_str() {
                    "DATE" => ParamValueKind::Date,
                    "DATE-TIME" => ParamValueKind::DateTime,
                    _ => ParamValueKind::Default,
                };
            }
        }
        ParamValueKind::Default
    }
}

/// Parse `YYYYMMDD` or `YYYYMMDDTHHMMSS[Z]` into (ms-since-epoch, has_time).
/// Treats no-Z as floating-UTC for now; a TZID-aware version is a follow-up.
fn parse_date_value(value: &str, kind: ParamValueKind) -> IcalResult<(i64, bool)> {
    let v = value.trim();
    let v = v.trim_end_matches('Z');
    if v.len() == 8 && kind != ParamValueKind::DateTime {
        // Date-only.
        let ymd = parse_ymd_compact(v)?;
        let days = ymd_to_days(ymd.0, ymd.1, ymd.2);
        Ok((days * 86_400 * 1000, false))
    } else if v.len() >= 15 && &v[8..9] == "T" {
        let ymd = parse_ymd_compact(&v[0..8])?;
        let h: i64 = v[9..11]
            .parse()
            .map_err(|_| IcalError::BadDate(value.to_string()))?;
        let m: i64 = v[11..13]
            .parse()
            .map_err(|_| IcalError::BadDate(value.to_string()))?;
        let s: i64 = v[13..15]
            .parse()
            .map_err(|_| IcalError::BadDate(value.to_string()))?;
        let days = ymd_to_days(ymd.0, ymd.1, ymd.2);
        let secs = days * 86_400 + h * 3600 + m * 60 + s;
        Ok((secs * 1000, true))
    } else {
        Err(IcalError::BadDate(value.to_string()))
    }
}

fn parse_ymd_compact(s: &str) -> IcalResult<(i32, u32, u32)> {
    if s.len() != 8 {
        return Err(IcalError::BadDate(s.to_string()));
    }
    let y: i32 = s[0..4].parse().map_err(|_| IcalError::BadDate(s.into()))?;
    let m: u32 = s[4..6].parse().map_err(|_| IcalError::BadDate(s.into()))?;
    let d: u32 = s[6..8].parse().map_err(|_| IcalError::BadDate(s.into()))?;
    Ok((y, m, d))
}

/// Parse an iCalendar DURATION value: `[+-]P[nW|nD][T[nH][nM][nS]]`.
/// Returns milliseconds (negative when prefixed with `-`).
fn parse_duration(value: &str) -> IcalResult<i64> {
    let v = value.trim();
    let (sign, rest) = if let Some(stripped) = v.strip_prefix('-') {
        (-1_i64, stripped)
    } else if let Some(stripped) = v.strip_prefix('+') {
        (1, stripped)
    } else {
        (1, v)
    };
    let rest = rest
        .strip_prefix('P')
        .ok_or_else(|| IcalError::BadDate(value.to_string()))?;

    let (date_part, time_part) = match rest.find('T') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };

    let mut total_secs: i64 = 0;
    let mut buf = String::new();
    for c in date_part.chars() {
        if c.is_ascii_digit() {
            buf.push(c);
        } else {
            let n: i64 = buf
                .parse()
                .map_err(|_| IcalError::BadDate(value.to_string()))?;
            buf.clear();
            match c {
                'W' => total_secs += n * 7 * 86_400,
                'D' => total_secs += n * 86_400,
                _ => return Err(IcalError::BadDate(value.to_string())),
            }
        }
    }
    if !buf.is_empty() {
        return Err(IcalError::BadDate(value.to_string()));
    }
    for c in time_part.chars() {
        if c.is_ascii_digit() {
            buf.push(c);
        } else {
            let n: i64 = buf
                .parse()
                .map_err(|_| IcalError::BadDate(value.to_string()))?;
            buf.clear();
            match c {
                'H' => total_secs += n * 3600,
                'M' => total_secs += n * 60,
                'S' => total_secs += n,
                _ => return Err(IcalError::BadDate(value.to_string())),
            }
        }
    }
    Ok(sign * total_secs * 1000)
}

fn format_duration(ms: i64) -> String {
    let mut out = String::with_capacity(16);
    if ms < 0 {
        out.push('-');
    }
    out.push('P');
    let mut secs = ms.unsigned_abs() / 1000;
    let days = secs / 86_400;
    secs %= 86_400;
    if days > 0 {
        let _ = write!(out, "{days}D");
    }
    if secs > 0 || days == 0 {
        out.push('T');
        let h = secs / 3600;
        secs %= 3600;
        let m = secs / 60;
        let s = secs % 60;
        if h > 0 {
            let _ = write!(out, "{h}H");
        }
        if m > 0 {
            let _ = write!(out, "{m}M");
        }
        if s > 0 || (h == 0 && m == 0) {
            let _ = write!(out, "{s}S");
        }
    }
    out
}

fn format_utc_datetime(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let day = secs.div_euclid(86_400);
    let secs_of_day = secs - day * 86_400;
    let (y, m, d) = days_to_ymd(day);
    let h = secs_of_day / 3600;
    let mi = (secs_of_day % 3600) / 60;
    let s = secs_of_day % 60;
    format!("{y:04}{m:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

fn format_date_property(name: &str, ms: i64, has_time: bool) -> String {
    if has_time {
        format!("{name}:{}", format_utc_datetime(ms))
    } else {
        let day = ms.div_euclid(1000).div_euclid(86_400);
        let (y, m, d) = days_to_ymd(day);
        format!("{name};VALUE=DATE:{y:04}{m:02}{d:02}")
    }
}

/// RFC 5545 line unfolding: a CRLF (or LF) followed by a SP/HTAB
/// continues the previous line.
fn unfold(input: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in input.split('\n') {
        let line = raw.trim_end_matches('\r');
        if let Some(continuation) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(prev) = out.last_mut() {
                prev.push_str(continuation);
                continue;
            }
        }
        out.push(line.to_string());
    }
    out
}

fn write_line(out: &mut String, line: &str) {
    // Fold at 75 octets per RFC 5545. Our content is ASCII-clamped in
    // practice (escape_text never produces multi-byte sequences from
    // ASCII inputs), so byte-length == octet-length.
    let mut start = 0usize;
    let bytes = line.as_bytes();
    while start < bytes.len() {
        let end = (start + 75).min(bytes.len());
        out.push_str(std::str::from_utf8(&bytes[start..end]).unwrap_or(""));
        out.push_str("\r\n");
        start = end;
        if start < bytes.len() {
            out.push(' ');
        }
    }
    if bytes.is_empty() {
        out.push_str("\r\n");
    }
}

/// Escape a TEXT value per RFC 5545: `,` `;` `\\` get backslash-
/// quoted; embedded newlines become `\n`.
fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            other => out.push(other),
        }
    }
    out
}

/// Inverse of `escape_text`. Tolerates `\N` as well (some clients).
fn unescape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some('\\') => out.push('\\'),
                Some(',') => out.push(','),
                Some(';') => out.push(';'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
PRODID:-//Tasks.org//Tasks//EN\r\n\
BEGIN:VTODO\r\n\
UID:abc-123\r\n\
SUMMARY:Buy milk\r\n\
DESCRIPTION:From the store\\, on the corner\r\n\
PRIORITY:5\r\n\
DTSTART:20240115T100000Z\r\n\
DUE:20240115T180000Z\r\n\
STATUS:NEEDS-ACTION\r\n\
RRULE:FREQ=WEEKLY;BYDAY=MO,WE,FR\r\n\
CATEGORIES:Work,Important\r\n\
RELATED-TO;RELTYPE=PARENT:parent-uid\r\n\
LAST-MODIFIED:20240116T120000Z\r\n\
CREATED:20240115T080000Z\r\n\
BEGIN:VALARM\r\n\
ACTION:DISPLAY\r\n\
TRIGGER:-PT30M\r\n\
END:VALARM\r\n\
END:VTODO\r\n\
END:VCALENDAR\r\n";

    #[test]
    fn parses_every_supported_property() {
        let v = parse_vcalendar(SAMPLE).unwrap();
        assert_eq!(v.uid, "abc-123");
        assert_eq!(v.summary.as_deref(), Some("Buy milk"));
        assert_eq!(
            v.description.as_deref(),
            Some("From the store, on the corner")
        );
        assert_eq!(v.priority, Some(5));
        assert_eq!(v.dtstart_ms, Some(1_705_312_800_000));
        assert!(v.dtstart_has_time);
        assert_eq!(v.due_ms, Some(1_705_341_600_000));
        assert!(v.due_has_time);
        assert_eq!(v.status, Some(VTodoStatus::NeedsAction));
        assert_eq!(v.rrule.as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,WE,FR"));
        assert_eq!(v.categories, vec!["Work", "Important"]);
        assert_eq!(v.parent_uid.as_deref(), Some("parent-uid"));
        assert_eq!(v.last_modified_ms, Some(1_705_406_400_000));
        assert_eq!(v.created_ms, Some(1_705_305_600_000));
        assert_eq!(v.alarms.len(), 1);
        assert_eq!(v.alarms[0].action, "DISPLAY");
        assert_eq!(v.alarms[0].trigger, AlarmTrigger::Offset(-30 * 60 * 1000));
    }

    #[test]
    fn date_only_due_is_recognised() {
        let blob = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\nDUE;VALUE=DATE:20240229\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let v = parse_vcalendar(blob).unwrap();
        assert!(!v.due_has_time);
        // 2024-02-29 UTC midnight = 1_709_164_800_000.
        assert_eq!(v.due_ms, Some(1_709_164_800_000));
    }

    #[test]
    fn round_trip_preserves_payload() {
        let original = parse_vcalendar(SAMPLE).unwrap();
        let serialised = serialize_vcalendar(&original);
        let reparsed = parse_vcalendar(&serialised).unwrap();
        assert_eq!(reparsed, original);
    }

    #[test]
    fn line_folding_reassembles() {
        // RFC 5545 line folding: a CRLF followed by a single space
        // (or HTAB) signals a continuation. Unfolder must drop the
        // CRLF + leading WS and concatenate.
        let folded = concat!(
            "BEGIN:VCALENDAR\r\n",
            "BEGIN:VTODO\r\n",
            "UID:x\r\n",
            "SUMMARY:abc\r\n",
            " def\r\n",
            "\tghi\r\n",
            "END:VTODO\r\n",
            "END:VCALENDAR\r\n",
        );
        let v = parse_vcalendar(folded).unwrap();
        assert_eq!(v.summary.as_deref(), Some("abcdefghi"));
    }

    #[test]
    fn missing_vtodo_errors_loudly() {
        let blob = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nEND:VCALENDAR\r\n";
        assert!(matches!(
            parse_vcalendar(blob).unwrap_err(),
            IcalError::NoVTodo
        ));
    }

    #[test]
    fn duration_round_trip() {
        for ms in [
            -30 * 60 * 1000,           // -30 minutes
            -60 * 60 * 1000,           // -1 hour
            7 * 86_400 * 1000,         // +7 days
            (7 * 24 + 30) * 60 * 1000, // 7h30m
            -86_400_000,               // -1 day
        ] {
            let s = format_duration(ms);
            assert_eq!(
                parse_duration(&s).unwrap(),
                ms,
                "round-trip failed for {ms}ms ({s})"
            );
        }
    }

    #[test]
    fn escape_text_round_trips_punctuation() {
        for input in [
            "no specials",
            "with, comma",
            "with; semi",
            "with\\backslash",
            "with\nnewline",
            "Pizza, beer; etc.",
        ] {
            let escaped = escape_text(input);
            assert_eq!(unescape_text(&escaped), input, "failed for {input:?}");
        }
    }

    #[test]
    fn absolute_trigger_is_recognised() {
        let blob = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\nBEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER;VALUE=DATE-TIME:20240115T180000Z\r\nEND:VALARM\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let v = parse_vcalendar(blob).unwrap();
        assert_eq!(v.alarms.len(), 1);
        assert_eq!(
            v.alarms[0].trigger,
            AlarmTrigger::Absolute(1_705_341_600_000)
        );
    }

    #[test]
    fn related_to_default_reltype_is_parent() {
        // RFC 5545 §3.2.15 — RELTYPE defaults to PARENT when absent.
        let blob = "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\nRELATED-TO:p\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let v = parse_vcalendar(blob).unwrap();
        assert_eq!(v.parent_uid.as_deref(), Some("p"));
    }

    /// M-3 input cap: a pathologically large VCALENDAR blob is
    /// rejected before we allocate line-by-line.
    #[test]
    fn too_large_input_is_rejected() {
        let blob = "X".repeat(MAX_INPUT_BYTES + 1);
        let err = parse_vcalendar(&blob).unwrap_err();
        match err {
            IcalError::TooLarge { bytes } => {
                assert_eq!(bytes, MAX_INPUT_BYTES + 1);
            }
            other => panic!("expected TooLarge, got {other:?}"),
        }
    }

    /// M-3 VALARM cap: more than MAX_ALARMS_PER_VTODO alarms fail
    /// the parse rather than silently dropping (or unbounded-
    /// allocating) the excess.
    #[test]
    fn too_many_valarms_are_rejected() {
        let mut blob = String::from("BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\n");
        for _ in 0..(MAX_ALARMS_PER_VTODO + 1) {
            blob.push_str("BEGIN:VALARM\r\nACTION:DISPLAY\r\nTRIGGER:-PT5M\r\nEND:VALARM\r\n");
        }
        blob.push_str("END:VTODO\r\nEND:VCALENDAR\r\n");
        let err = parse_vcalendar(&blob).unwrap_err();
        assert!(
            matches!(err, IcalError::TooMany { field: "VALARM" }),
            "expected TooMany{{VALARM}}, got {err:?}"
        );
    }

    /// L-2: malformed parameter without `=` is rejected.
    #[test]
    fn property_parameter_without_equals_is_rejected() {
        // `UID;FOO:value` has a parameter chunk `FOO` with no `=`.
        // RFC 5545 §3.2 requires `param-name = param-value`.
        let blob =
            "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID;FOO:value\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let err = parse_vcalendar(blob).unwrap_err();
        assert!(matches!(err, IcalError::MalformedLine(_)));
    }

    /// L-2: an empty parameter name (e.g. `UID;=val:value`) is
    /// malformed.
    #[test]
    fn property_empty_param_name_is_rejected() {
        let blob =
            "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID;=x:value\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let err = parse_vcalendar(blob).unwrap_err();
        assert!(matches!(err, IcalError::MalformedLine(_)));
    }

    /// L-2: a line whose property name is empty (leading `:`) is
    /// rejected instead of silently dropped.
    #[test]
    fn property_empty_name_is_rejected() {
        // `:value` inside a VTODO — the old parser would have
        // up-cased the empty name and kept walking.
        let blob =
            "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\n:value\r\nEND:VTODO\r\nEND:VCALENDAR\r\n";
        let err = parse_vcalendar(blob).unwrap_err();
        assert!(matches!(err, IcalError::MalformedLine(_)));
    }

    /// L-1: a VCALENDAR with two VTODO blocks fails the parse
    /// instead of silently returning just the first. CalDAV
    /// multi-status responses wrap each task in its own envelope,
    /// so this shape only ever shows up as server misbehaviour.
    #[test]
    fn multiple_vtodos_are_rejected() {
        let blob = concat!(
            "BEGIN:VCALENDAR\r\n",
            "BEGIN:VTODO\r\nUID:one\r\nSUMMARY:First\r\nEND:VTODO\r\n",
            "BEGIN:VTODO\r\nUID:two\r\nSUMMARY:Second\r\nEND:VTODO\r\n",
            "END:VCALENDAR\r\n",
        );
        let err = parse_vcalendar(blob).unwrap_err();
        assert!(
            matches!(err, IcalError::MultipleVtodos),
            "expected MultipleVtodos, got {err:?}"
        );
    }

    /// M-3 CATEGORIES cap: more than MAX_CATEGORIES_PER_VTODO tags
    /// fail the parse.
    #[test]
    fn too_many_categories_are_rejected() {
        let tags: Vec<String> = (0..(MAX_CATEGORIES_PER_VTODO + 5))
            .map(|i| format!("tag{i}"))
            .collect();
        let cats = tags.join(",");
        let blob = format!(
            "BEGIN:VCALENDAR\r\nBEGIN:VTODO\r\nUID:x\r\nCATEGORIES:{cats}\r\nEND:VTODO\r\nEND:VCALENDAR\r\n"
        );
        let err = parse_vcalendar(&blob).unwrap_err();
        assert!(
            matches!(
                err,
                IcalError::TooMany {
                    field: "CATEGORIES"
                }
            ),
            "expected TooMany{{CATEGORIES}}, got {err:?}"
        );
    }
}
