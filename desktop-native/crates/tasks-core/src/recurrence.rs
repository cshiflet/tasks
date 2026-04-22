//! RRULE humanizer.
//!
//! Tasks.org stores recurrences as RFC 5545 RRULE strings in
//! `tasks.recurrence` (e.g. `FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR`).
//! The raw form is self-explanatory to engineers but opaque to users,
//! so we render a plain-English summary for the detail pane.
//!
//! This is a deliberately partial parser — it understands the parts
//! of RRULE that show up in 95 % of real tasks (FREQ, INTERVAL,
//! BYDAY, COUNT, UNTIL) and silently drops the positional prefixes
//! on BYDAY and the less common parts (BYMONTHDAY, BYSETPOS, WKST).
//! When we can't parse even the frequency, we fall back to echoing
//! the raw rule — partial knowledge beats no knowledge, but we never
//! pretend to understand a rule we don't.
//!
//! A full port of Android's `RepeatRuleToString` (locale-aware,
//! supports every BY-part) is a later milestone; this module exists
//! to get recurring tasks out of the "FREQ=DAILY;INTERVAL=1" zone
//! without a dependency on the `rrule` crate.
//!
//! The `repeat_from_completion` flag is the content of
//! `tasks.repeat_from` (0 = from due date, 1 = from completion). It
//! materially changes behaviour (a "daily" task from due date recurs
//! on the *schedule*, one from completion recurs N days after the
//! user ticks the box), so we always surface the difference in the
//! output.

/// Humanise an RRULE.
///
/// Returns an empty string when `rrule` is empty. Falls back to the
/// raw RRULE (with the from-completion suffix when appropriate) when
/// the frequency is absent or unrecognised, so callers can safely
/// render the result verbatim.
pub fn humanize_rrule(rrule: &str, repeat_from_completion: bool) -> String {
    if rrule.is_empty() {
        return String::new();
    }

    let mut freq: Option<&str> = None;
    let mut interval: u32 = 1;
    let mut by_day: Option<&str> = None;
    let mut by_month_day: Option<&str> = None;
    let mut until: Option<&str> = None;
    let mut count: Option<u32> = None;

    // RRULE parts are `KEY=VALUE` tuples separated by `;`. Some
    // variants (rare) prefix the whole rule with `RRULE:` — strip
    // that defensively so we don't fail the FREQ detection on
    // `RRULE:FREQ=DAILY`.
    let body = rrule.strip_prefix("RRULE:").unwrap_or(rrule);
    for part in body.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim() {
            "FREQ" => freq = Some(value.trim()),
            "INTERVAL" => {
                if let Ok(n) = value.trim().parse::<u32>() {
                    if n > 0 {
                        interval = n;
                    }
                }
            }
            "BYDAY" => by_day = Some(value.trim()),
            "BYMONTHDAY" => by_month_day = Some(value.trim()),
            "UNTIL" => until = Some(value.trim()),
            "COUNT" => count = value.trim().parse::<u32>().ok(),
            _ => {} // Ignore BYSETPOS / WKST / BYYEARDAY / etc.
        }
    }

    let Some(freq) = freq else {
        return with_from_suffix(rrule, repeat_from_completion);
    };

    let unit_singular = match freq {
        "SECONDLY" => "second",
        "MINUTELY" => "minute",
        "HOURLY" => "hour",
        "DAILY" => "day",
        "WEEKLY" => "week",
        "MONTHLY" => "month",
        "YEARLY" => "year",
        _ => return with_from_suffix(rrule, repeat_from_completion),
    };

    let mut out = match interval {
        1 => format!("Every {unit_singular}"),
        2 => format!("Every other {unit_singular}"),
        n => format!("Every {n} {unit_singular}s"),
    };

    if let Some(days) = by_day {
        let names = format_by_day(days);
        if !names.is_empty() {
            out.push_str(" on ");
            out.push_str(&names);
        }
    }

    if let Some(days) = by_month_day {
        let rendered = format_by_month_day(days);
        if !rendered.is_empty() {
            out.push_str(" on the ");
            out.push_str(&rendered);
        }
    }

    if let Some(n) = count {
        out.push_str(&format!(", {n} times"));
    } else if let Some(u) = until {
        if let Some(formatted) = format_until(u) {
            out.push_str(" until ");
            out.push_str(&formatted);
        }
    }

    if repeat_from_completion {
        out.push_str(" (from completion)");
    }
    out
}

/// Format BYDAY's comma-separated day codes, now with positional
/// prefix support. "MO,WE,FR" → "Mon, Wed, Fri"; "-1FR" → "last
/// Friday"; "1MO" → "first Monday". Mixing positional and plain
/// items works — "-1FR,MO" → "last Friday, Mon" — though Android
/// itself rarely produces that combination.
fn format_by_day(by_day: &str) -> String {
    let mut items: Vec<String> = Vec::new();
    for raw in by_day.split(',') {
        let raw = raw.trim();
        // Split into numeric prefix (with optional sign) + two-letter
        // weekday code.
        let prefix_len = raw
            .find(|c: char| c.is_ascii_alphabetic())
            .unwrap_or(raw.len());
        let prefix = &raw[..prefix_len];
        let code = &raw[prefix_len..];
        let day_name = match code {
            "MO" => "Mon",
            "TU" => "Tue",
            "WE" => "Wed",
            "TH" => "Thu",
            "FR" => "Fri",
            "SA" => "Sat",
            "SU" => "Sun",
            _ => continue,
        };
        if prefix.is_empty() {
            items.push(day_name.to_string());
        } else if let Ok(pos) = prefix.parse::<i32>() {
            items.push(format!("{} {day_name}", ordinal_prefix(pos)));
        } else {
            // Unparseable prefix; fall back to the plain weekday.
            items.push(day_name.to_string());
        }
    }
    items.join(", ")
}

/// Format BYMONTHDAY: one or more comma-separated day-of-month
/// values (1..31, or -1 = last, -2 = second-to-last, etc.).
fn format_by_month_day(by_month_day: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for raw in by_month_day.split(',') {
        let raw = raw.trim();
        if let Ok(n) = raw.parse::<i32>() {
            if n > 0 {
                parts.push(ordinal_day(n as u32));
            } else if n == -1 {
                parts.push("last day".to_string());
            } else if n < 0 {
                parts.push(format!("{}-to-last day", ordinal_prefix(-n - 1)));
            }
        }
    }
    parts.join(", ")
}

/// Render a positional index from RRULE (1 = first, -1 = last,
/// 2 = second, -2 = second-to-last, …) as an English phrase.
fn ordinal_prefix(n: i32) -> String {
    match n {
        1 => "first".to_string(),
        2 => "second".to_string(),
        3 => "third".to_string(),
        4 => "fourth".to_string(),
        5 => "fifth".to_string(),
        -1 => "last".to_string(),
        -2 => "second-to-last".to_string(),
        -3 => "third-to-last".to_string(),
        other if other > 0 => format!("{other}th"),
        other => format!("{}-to-last", -other - 1),
    }
}

/// Render a 1-based day-of-month as "1st", "2nd", "3rd", "4th", …
fn ordinal_day(n: u32) -> String {
    let suffix = match (n % 100, n % 10) {
        (11..=13, _) => "th",
        (_, 1) => "st",
        (_, 2) => "nd",
        (_, 3) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// RRULE UNTIL is either `YYYYMMDD` (date) or `YYYYMMDDTHHMMSSZ`
/// (UTC date-time). We render the date portion as ISO-8601.
fn format_until(until: &str) -> Option<String> {
    if until.len() < 8 {
        return None;
    }
    let y: &str = &until[0..4];
    let m: &str = &until[4..6];
    let d: &str = &until[6..8];
    if !y.chars().all(|c| c.is_ascii_digit())
        || !m.chars().all(|c| c.is_ascii_digit())
        || !d.chars().all(|c| c.is_ascii_digit())
    {
        return None;
    }
    Some(format!("{y}-{m}-{d}"))
}

fn with_from_suffix(raw: &str, from_completion: bool) -> String {
    if from_completion {
        format!("{raw} (from completion)")
    } else {
        raw.to_string()
    }
}

/// Compute the next occurrence for a recurring task after completion.
///
/// Mirrors the Android client's "advance on complete" behaviour: a
/// daily task ticked once advances `dueDate` to tomorrow rather
/// than stamping `completed`. Handles FREQ / INTERVAL / BYDAY
/// (WEEKLY). COUNT and UNTIL are parsed but silently ignored (the
/// caller can layer termination on top once we add termination to
/// the edit dialog).
///
/// Arguments:
/// * `rrule` — the task's RRULE string; empty or unparseable
///   returns `None`.
/// * `due_ms` — the task's current `dueDate` in ms since epoch.
/// * `completed_at_ms` — the moment the user ticked the box.
/// * `from_completion` — `true` if `repeat_from == COMPLETION_DATE`.
///
/// Returns the next occurrence's due-date in ms, or `None` when
/// the rule can't be advanced (unknown FREQ, a task with no
/// `dueDate`, zero interval, etc.). When `None`, callers should
/// fall back to the "stamp completed" path.
pub fn advance_recurrence(
    rrule: &str,
    due_ms: i64,
    completed_at_ms: i64,
    from_completion: bool,
) -> Option<i64> {
    if rrule.is_empty() || due_ms == 0 {
        return None;
    }

    let mut freq: Option<&str> = None;
    let mut interval: u32 = 1;
    let mut by_day: Vec<u8> = Vec::new();

    let body = rrule.strip_prefix("RRULE:").unwrap_or(rrule);
    for part in body.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "FREQ" => freq = Some(value),
            "INTERVAL" => {
                if let Ok(n) = value.parse::<u32>() {
                    if n > 0 {
                        interval = n;
                    }
                }
            }
            "BYDAY" => {
                for raw in value.split(',') {
                    let trimmed = raw
                        .trim()
                        .trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit());
                    // MO=0, TU=1, WE=2, TH=3, FR=4, SA=5, SU=6
                    let code: Option<u8> = match trimmed {
                        "MO" => Some(0),
                        "TU" => Some(1),
                        "WE" => Some(2),
                        "TH" => Some(3),
                        "FR" => Some(4),
                        "SA" => Some(5),
                        "SU" => Some(6),
                        _ => None,
                    };
                    if let Some(c) = code {
                        if !by_day.contains(&c) {
                            by_day.push(c);
                        }
                    }
                }
                by_day.sort();
            }
            _ => {}
        }
    }

    let freq = freq?;

    // Base point for the advance. When repeat_from=completion, the
    // next due date is `interval × unit` after completion; for
    // repeat_from=due, we advance relative to the current due. Both
    // modes preserve the time-of-day from `due_ms`.
    let base_ms = if from_completion {
        // Preserve due's time-of-day but anchor the day to completion.
        let due_secs = due_ms / 1000;
        let due_day = due_secs.div_euclid(86_400);
        let due_time = due_secs - due_day * 86_400;
        let comp_day = (completed_at_ms / 1000).div_euclid(86_400);
        (comp_day * 86_400 + due_time) * 1000
    } else {
        due_ms
    };

    match freq {
        "DAILY" => Some(add_days(base_ms, interval as i64)),
        "WEEKLY" => {
            if by_day.is_empty() {
                Some(add_days(base_ms, interval as i64 * 7))
            } else {
                Some(next_weekly_byday(base_ms, interval, &by_day))
            }
        }
        "MONTHLY" => Some(add_months(base_ms, interval as i64)),
        "YEARLY" => Some(add_months(base_ms, interval as i64 * 12)),
        "HOURLY" => Some(base_ms + interval as i64 * 3_600_000),
        "MINUTELY" => Some(base_ms + interval as i64 * 60_000),
        _ => None,
    }
}

/// Add whole days, keeping the time-of-day component intact.
fn add_days(ms: i64, days: i64) -> i64 {
    ms + days * 86_400_000
}

/// Add `months_delta` calendar months to `ms`, clamping the day
/// to whatever the destination month allows (Jan 31 + 1 month →
/// Feb 28 in a non-leap year). Time-of-day is preserved.
fn add_months(ms: i64, months_delta: i64) -> i64 {
    use crate::datetime::{days_to_ymd, ymd_to_days};
    let secs = ms / 1000;
    let day = secs.div_euclid(86_400);
    let time_of_day = secs - day * 86_400;
    let (y, m, d) = days_to_ymd(day);

    let mut month0 = m as i64 - 1 + months_delta;
    let mut year = y as i64 + month0.div_euclid(12);
    month0 = month0.rem_euclid(12);
    let month = (month0 + 1) as u32;
    let max_day = days_in_month(year as i32, month);
    let day_of_month = d.min(max_day);

    // year fits into i32 for any realistic recurrence horizon
    if year > i32::MAX as i64 {
        year = i32::MAX as i64;
    }
    let days = ymd_to_days(year as i32, month, day_of_month);
    (days * 86_400 + time_of_day) * 1000
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
            if leap {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

/// Next occurrence for `FREQ=WEEKLY;BYDAY=…`. `by_day` values are
/// 0=MO … 6=SU, sorted ascending. We find the next BYDAY strictly
/// after the base's weekday; if there isn't one later in the
/// current week, jump to the first BYDAY of the week
/// `interval` steps forward.
fn next_weekly_byday(base_ms: i64, interval: u32, by_day: &[u8]) -> i64 {
    // 1970-01-01 was a Thursday → day 0 has weekday TH (index 3).
    // Mon=0, Tue=1, ..., Sun=6.
    let day_from_epoch = (base_ms / 1000).div_euclid(86_400);
    let weekday = ((day_from_epoch + 3).rem_euclid(7)) as u8; // 0=MO … 6=SU
    for &target in by_day {
        if target > weekday {
            return add_days(base_ms, (target - weekday) as i64);
        }
    }
    // No remaining BYDAY this week: jump to next interval × week, pick the first BYDAY.
    let first = by_day.first().copied().unwrap_or(weekday);
    let days_to_next_week_start = 7 - weekday as i64;
    let step_days = days_to_next_week_start + (interval as i64 - 1) * 7;
    add_days(base_ms, step_days + first as i64)
}

#[cfg(test)]
mod tests {
    use super::{advance_recurrence, humanize_rrule};

    // 2024-01-15 00:00:00 UTC, a Monday.
    const MON_2024_01_15: i64 = 1_705_276_800_000;
    // 2024-02-29 (leap day), 12:00:00 UTC.
    const FEB_29_2024_NOON: i64 = 1_709_208_000_000;

    #[test]
    fn advance_daily_adds_interval_days() {
        assert_eq!(
            advance_recurrence("FREQ=DAILY", MON_2024_01_15, 0, false),
            Some(MON_2024_01_15 + 86_400_000)
        );
        assert_eq!(
            advance_recurrence("FREQ=DAILY;INTERVAL=3", MON_2024_01_15, 0, false),
            Some(MON_2024_01_15 + 3 * 86_400_000)
        );
    }

    #[test]
    fn advance_weekly_without_byday_adds_weeks() {
        assert_eq!(
            advance_recurrence("FREQ=WEEKLY;INTERVAL=2", MON_2024_01_15, 0, false),
            Some(MON_2024_01_15 + 14 * 86_400_000)
        );
    }

    #[test]
    fn advance_weekly_with_byday_finds_next_weekday() {
        // Monday → next Wednesday (2 days later).
        assert_eq!(
            advance_recurrence("FREQ=WEEKLY;BYDAY=MO,WE,FR", MON_2024_01_15, 0, false),
            Some(MON_2024_01_15 + 2 * 86_400_000)
        );
        // Wednesday → Friday (2 days later).
        let wed = MON_2024_01_15 + 2 * 86_400_000;
        assert_eq!(
            advance_recurrence("FREQ=WEEKLY;BYDAY=MO,WE,FR", wed, 0, false),
            Some(wed + 2 * 86_400_000)
        );
        // Friday (no Sat/Sun in rule) → next Monday (3 days later).
        let fri = MON_2024_01_15 + 4 * 86_400_000;
        assert_eq!(
            advance_recurrence("FREQ=WEEKLY;BYDAY=MO,WE,FR", fri, 0, false),
            Some(fri + 3 * 86_400_000)
        );
        // Friday with INTERVAL=2 → two-week skip: 3 days + 7 = 10 days.
        assert_eq!(
            advance_recurrence("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,WE,FR", fri, 0, false),
            Some(fri + 10 * 86_400_000)
        );
    }

    #[test]
    fn advance_monthly_clamps_to_month_length() {
        // Jan 31 + 1 month → Feb 29 (2024 leap year).
        let jan_31_2024 = 1_706_659_200_000;
        assert_eq!(
            advance_recurrence("FREQ=MONTHLY", jan_31_2024, 0, false),
            Some(FEB_29_2024_NOON - 12 * 3_600_000) // midnight Feb 29
        );
        // Feb 29 2024 + 1 month → Mar 29. Then + 1 month → Apr 29.
        let next = advance_recurrence("FREQ=MONTHLY", FEB_29_2024_NOON, 0, false).unwrap();
        // March has 31 days so day stays 29, time of day (noon) preserved.
        let mar_29_2024_noon = 1_711_713_600_000;
        assert_eq!(next, mar_29_2024_noon);
    }

    #[test]
    fn advance_yearly_handles_leap_feb_29() {
        // Feb 29 2024 + 1 year → Feb 28 2025 (non-leap).
        let feb_28_2025_noon = 1_740_744_000_000;
        assert_eq!(
            advance_recurrence("FREQ=YEARLY", FEB_29_2024_NOON, 0, false),
            Some(feb_28_2025_noon)
        );
    }

    #[test]
    fn advance_with_repeat_from_completion_anchors_to_completion_day() {
        // Due was Jan 15 2024 @ noon, user completes Jan 20 2024 @ 14:00.
        // Next due should be completion day + interval, with the
        // original time-of-day (noon) preserved.
        let due = MON_2024_01_15 + 12 * 3_600_000;
        let completed = MON_2024_01_15 + 5 * 86_400_000 + 14 * 3_600_000;
        // FREQ=DAILY → next due = completion_day + 1 @ due's time of day
        // = Jan 21 @ noon.
        let expected = MON_2024_01_15 + 6 * 86_400_000 + 12 * 3_600_000;
        assert_eq!(
            advance_recurrence("FREQ=DAILY", due, completed, true),
            Some(expected)
        );
    }

    #[test]
    fn advance_unknown_or_missing_returns_none() {
        assert_eq!(advance_recurrence("", MON_2024_01_15, 0, false), None);
        assert_eq!(advance_recurrence("FREQ=DAILY", 0, 0, false), None);
        assert_eq!(
            advance_recurrence("FREQ=FORTNIGHTLY", MON_2024_01_15, 0, false),
            None
        );
        // Missing FREQ.
        assert_eq!(
            advance_recurrence("INTERVAL=2", MON_2024_01_15, 0, false),
            None
        );
    }

    #[test]
    fn empty_rule_stays_empty() {
        assert_eq!(humanize_rrule("", false), "");
        assert_eq!(humanize_rrule("", true), "");
    }

    #[test]
    fn daily_weekly_monthly_yearly() {
        assert_eq!(humanize_rrule("FREQ=DAILY", false), "Every day");
        assert_eq!(humanize_rrule("FREQ=WEEKLY", false), "Every week");
        assert_eq!(humanize_rrule("FREQ=MONTHLY", false), "Every month");
        assert_eq!(humanize_rrule("FREQ=YEARLY", false), "Every year");
        assert_eq!(humanize_rrule("FREQ=HOURLY", false), "Every hour");
    }

    #[test]
    fn interval_reads_as_english() {
        assert_eq!(humanize_rrule("FREQ=DAILY;INTERVAL=1", false), "Every day");
        assert_eq!(
            humanize_rrule("FREQ=DAILY;INTERVAL=2", false),
            "Every other day"
        );
        assert_eq!(
            humanize_rrule("FREQ=DAILY;INTERVAL=3", false),
            "Every 3 days"
        );
        assert_eq!(
            humanize_rrule("FREQ=WEEKLY;INTERVAL=2", false),
            "Every other week"
        );
    }

    #[test]
    fn by_day_appends_weekday_list() {
        assert_eq!(
            humanize_rrule("FREQ=WEEKLY;BYDAY=MO,WE,FR", false),
            "Every week on Mon, Wed, Fri"
        );
        assert_eq!(
            humanize_rrule("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH", false),
            "Every other week on Tue, Thu"
        );
    }

    #[test]
    fn by_day_renders_positional_prefixes() {
        // "-1FR" → "last Friday", "1MO" → "first Monday".
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYDAY=-1FR", false),
            "Every month on last Fri"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYDAY=1MO", false),
            "Every month on first Mon"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYDAY=2TU", false),
            "Every month on second Tue"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYDAY=-2SU", false),
            "Every month on second-to-last Sun"
        );
    }

    #[test]
    fn by_month_day_renders_as_ordinal() {
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYMONTHDAY=1", false),
            "Every month on the 1st"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYMONTHDAY=15", false),
            "Every month on the 15th"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYMONTHDAY=1,15", false),
            "Every month on the 1st, 15th"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYMONTHDAY=-1", false),
            "Every month on the last day"
        );
        // The teens all take "th" (11th, 12th, 13th) not "st/nd/rd".
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYMONTHDAY=11,12,13", false),
            "Every month on the 11th, 12th, 13th"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYMONTHDAY=21,22,23", false),
            "Every month on the 21st, 22nd, 23rd"
        );
    }

    #[test]
    fn count_and_until_append_scope() {
        assert_eq!(
            humanize_rrule("FREQ=DAILY;COUNT=5", false),
            "Every day, 5 times"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;UNTIL=20260601", false),
            "Every month until 2026-06-01"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;UNTIL=20260601T120000Z", false),
            "Every month until 2026-06-01"
        );
    }

    #[test]
    fn repeat_from_completion_appends_suffix() {
        assert_eq!(
            humanize_rrule("FREQ=DAILY", true),
            "Every day (from completion)"
        );
        assert_eq!(
            humanize_rrule("FREQ=WEEKLY;BYDAY=MO", true),
            "Every week on Mon (from completion)"
        );
    }

    #[test]
    fn unparseable_falls_back_to_raw() {
        // No FREQ → raw.
        assert_eq!(humanize_rrule("BYDAY=MO", false), "BYDAY=MO");
        // Unknown FREQ → raw.
        assert_eq!(
            humanize_rrule("FREQ=FORTNIGHTLY", false),
            "FREQ=FORTNIGHTLY"
        );
        // Raw fallback still picks up the from-completion suffix so
        // the user sees it regardless.
        assert_eq!(
            humanize_rrule("FREQ=FORTNIGHTLY", true),
            "FREQ=FORTNIGHTLY (from completion)"
        );
    }

    #[test]
    fn rrule_prefix_is_tolerated() {
        assert_eq!(humanize_rrule("RRULE:FREQ=DAILY", false), "Every day");
    }
}
