//! Epoch-millisecond ↔ `YYYY-MM-DD [HH:MM]` helpers.
//!
//! Tasks.org stores due dates and hide-until dates as milliseconds
//! since the Unix epoch in the `tasks` table. The GUI needs to
//! display and accept them as text. Both directions live here so the
//! detail pane's read path and the edit dialog's write path can't
//! drift out of sync.
//!
//! Semantics this module preserves:
//!
//! * **Date-only vs date+time**. The Android client flags timed tasks
//!   with a non-zero seconds component (`secs % 60 != 0`). A typed
//!   "HH:MM" field gets a `+1 second` flag tacked on so the same file
//!   opened from Android still reads as timed. Date-only typed
//!   entries stamp midnight exactly.
//! * **UTC everywhere** for now. Both display and parse interpret
//!   the string as UTC, so edits round-trip bit-for-bit even though
//!   the user probably means local time. A timezone-correct pass is
//!   a follow-up (see `PLAN_UPDATES.md §2.4`).
//! * **Empty ↔ 0**. The tasks table uses `0` to mean "no date";
//!   `format_due_label(0)` returns an empty string and
//!   `parse_due_input("")` returns `Ok(0)`.

/// Format a millisecond-epoch date as a compact UTC string.
/// Returns an empty string when `due_ms <= 0` (no date set).
pub fn format_due_label(due_ms: i64) -> String {
    if due_ms <= 0 {
        return String::new();
    }
    let secs = due_ms / 1000;
    let days_from_epoch = secs.div_euclid(86_400);
    let (y, m, d) = days_to_ymd(days_from_epoch);
    let has_time = secs % 60 > 0;
    if has_time {
        let secs_of_day = secs - days_from_epoch * 86_400;
        let h = secs_of_day / 3600;
        let min = (secs_of_day % 3600) / 60;
        format!("{y:04}-{m:02}-{d:02} {h:02}:{min:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// Parse a user-typed `YYYY-MM-DD [HH:MM]` into a millisecond-epoch
/// UTC timestamp. Returns `Ok(0)` for an empty / whitespace-only
/// input (caller semantics: "no date"). Returns `Err` with a short
/// user-facing phrase on malformed input so the bridge can pass it
/// straight to the status bar.
///
/// Accepted shapes:
/// * `""` (empty) → 0
/// * `YYYY-MM-DD` → that day at 00:00:00 UTC (date-only)
/// * `YYYY-MM-DD HH:MM` → that day at HH:MM:01 UTC (seconds == 1
///   marks the "has time" flag, matching Android's writer)
///
/// The `T` separator (ISO 8601) is accepted too so a paste from an
/// external tool doesn't trip the parser.
pub fn parse_due_input(s: &str) -> Result<i64, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(0);
    }

    // Split on either ' ' or 'T'; keeping date and time portions.
    let (date_part, time_part) = match s.find([' ', 'T']) {
        Some(i) => {
            let tp = s[i + 1..].trim();
            let tp = tp.trim_end_matches('Z'); // tolerate trailing Z
            (&s[..i], if tp.is_empty() { None } else { Some(tp) })
        }
        None => (s, None),
    };

    let (y, m, d) = parse_ymd(date_part)?;
    let days = ymd_to_days(y, m, d);
    let mut secs: i64 = days * 86_400;

    if let Some(tp) = time_part {
        let (h, mi) = parse_hm(tp)?;
        // +1 second flags "has time" — matches the Android
        // `Task.hasDueTime` convention.
        secs += h as i64 * 3600 + mi as i64 * 60 + 1;
    }

    Ok(secs * 1000)
}

fn parse_ymd(s: &str) -> Result<(i32, u32, u32), String> {
    let parts: Vec<&str> = s.split('-').collect();
    if parts.len() != 3 {
        return Err(format!("expected YYYY-MM-DD, got \"{s}\""));
    }
    let y: i32 = parts[0]
        .parse()
        .map_err(|_| format!("year not a number: \"{}\"", parts[0]))?;
    let m: u32 = parts[1]
        .parse()
        .map_err(|_| format!("month not a number: \"{}\"", parts[1]))?;
    let d: u32 = parts[2]
        .parse()
        .map_err(|_| format!("day not a number: \"{}\"", parts[2]))?;
    if !(1..=12).contains(&m) {
        return Err(format!("month out of range: {m}"));
    }
    if !(1..=31).contains(&d) {
        return Err(format!("day out of range: {d}"));
    }
    Ok((y, m, d))
}

fn parse_hm(s: &str) -> Result<(u32, u32), String> {
    // Accept "HH:MM" or "HH:MM:SS" (we drop the seconds — the
    // "has time" flag is added unconditionally by the caller).
    let parts: Vec<&str> = s.split(':').collect();
    if !(parts.len() == 2 || parts.len() == 3) {
        return Err(format!("expected HH:MM, got \"{s}\""));
    }
    let h: u32 = parts[0]
        .parse()
        .map_err(|_| format!("hour not a number: \"{}\"", parts[0]))?;
    let mi: u32 = parts[1]
        .parse()
        .map_err(|_| format!("minute not a number: \"{}\"", parts[1]))?;
    if h >= 24 {
        return Err(format!("hour out of range: {h}"));
    }
    if mi >= 60 {
        return Err(format!("minute out of range: {mi}"));
    }
    Ok((h, mi))
}

/// Convert Unix-epoch day count to `(year, month, day)` in the
/// proleptic Gregorian calendar. Howard Hinnant's `civil_from_days`
/// with era-offset 719468 days (0000-03-01 → 1970-01-01). Accurate
/// from -1 000 000 to 1 000 000 AD — plenty for any Tasks.org row.
pub fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Inverse of `days_to_ymd`. Returns the count of days from the
/// Unix epoch to the given Gregorian date.
pub fn ymd_to_days(y: i32, m: u32, d: u32) -> i64 {
    // Hinnant's `days_from_civil`, expressed with i64 throughout so
    // the signed arithmetic stays well-defined for y < 0.
    let y = if m <= 2 { y as i64 - 1 } else { y as i64 };
    let era = y.div_euclid(400);
    let yoe = (y - era * 400) as u64; // [0, 399]
    let m = m as u64;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + (d as u64) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe as i64 - 719_468
}

#[cfg(test)]
mod tests {
    use super::{days_to_ymd, format_due_label, parse_due_input, ymd_to_days};

    #[test]
    fn days_to_ymd_round_trip_known_values() {
        assert_eq!(days_to_ymd(0), (1970, 1, 1));
        assert_eq!(days_to_ymd(10957), (2000, 1, 1));
        assert_eq!(days_to_ymd(18321), (2020, 2, 29));
    }

    #[test]
    fn ymd_to_days_inverts_days_to_ymd() {
        for days in [0_i64, 1, -1, 365, 365 * 30, 365 * 100, 18_321, 25_000] {
            let (y, m, d) = days_to_ymd(days);
            assert_eq!(ymd_to_days(y, m, d), days, "round trip failed for {days}");
        }
    }

    #[test]
    fn format_due_label_date_only_vs_datetime() {
        // Midnight UTC 2020-02-29 → date-only.
        assert_eq!(format_due_label(1_582_934_400_000), "2020-02-29");
        // Non-zero seconds flags "has time".
        assert_eq!(format_due_label(1_582_979_641_000), "2020-02-29 12:34");
        assert_eq!(format_due_label(0), "");
        assert_eq!(format_due_label(-1), "");
    }

    #[test]
    fn parse_due_input_empty_yields_zero() {
        assert_eq!(parse_due_input(""), Ok(0));
        assert_eq!(parse_due_input("   "), Ok(0));
    }

    #[test]
    fn parse_due_input_date_only_is_midnight_utc() {
        assert_eq!(parse_due_input("2020-02-29"), Ok(1_582_934_400_000));
        assert_eq!(parse_due_input("1970-01-01"), Ok(0)); // Empty would also give 0, but this is a date.
                                                          // Nit: "1970-01-01" and "" both parse to 0 — semantically
                                                          // "no date" vs "the epoch"; callers treating 0 as "no date"
                                                          // (which matches Tasks.org semantics) are unaffected.
    }

    #[test]
    fn parse_due_input_date_time_rounds_to_has_time_flag() {
        // "12:34" → :01 seconds to flag has-time.
        let expected = 1_582_934_400_000 + (12 * 3600 + 34 * 60 + 1) * 1000;
        assert_eq!(parse_due_input("2020-02-29 12:34"), Ok(expected));
        // ISO-8601 T separator is accepted.
        assert_eq!(parse_due_input("2020-02-29T12:34"), Ok(expected));
        assert_eq!(parse_due_input("2020-02-29T12:34Z"), Ok(expected));
    }

    #[test]
    fn parse_due_input_round_trips_format_due_label() {
        // "HH:MM:01 flag" round-trips through the formatter even
        // though the formatter drops seconds.
        for ms in [0_i64, 1_582_934_400_000, 1_582_979_641_000] {
            let text = format_due_label(ms);
            let parsed = parse_due_input(&text).unwrap();
            // 0-case: text is empty, parse gives 0; match.
            // Timed case: parse output includes +1 second, original
            // ms also had a non-zero seconds component → both round
            // to the same "HH:MM" display, but the raw ms may
            // differ. Assert the *display* matches, not the raw ms.
            assert_eq!(format_due_label(parsed), text);
        }
    }

    #[test]
    fn parse_due_input_rejects_malformed_input() {
        assert!(parse_due_input("not-a-date").is_err());
        assert!(parse_due_input("2020-13-01").is_err());
        assert!(parse_due_input("2020-02-30 99:99").is_err());
        assert!(parse_due_input("2020-02-29 25:00").is_err());
        assert!(parse_due_input("2020-02-29 12:70").is_err());
        assert!(parse_due_input("2020/02/29").is_err());
    }
}
