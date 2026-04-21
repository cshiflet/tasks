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
            "UNTIL" => until = Some(value.trim()),
            "COUNT" => count = value.trim().parse::<u32>().ok(),
            _ => {} // Ignore BYMONTHDAY / BYSETPOS / WKST / etc.
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

/// Format BYDAY's comma-separated day codes. Positional prefixes
/// ("-1FR" = last Friday, "1MO" = first Monday) are dropped along
/// with any unrecognised codes — the nuance belongs in the full
/// Android-parity port, not here.
fn format_by_day(by_day: &str) -> String {
    let mut names = Vec::new();
    for raw in by_day.split(',') {
        // Strip leading sign and digits: "-1FR" → "FR", "1MO" → "MO".
        let trimmed = raw
            .trim()
            .trim_start_matches(|c: char| c == '-' || c == '+' || c.is_ascii_digit());
        let name = match trimmed {
            "MO" => "Mon",
            "TU" => "Tue",
            "WE" => "Wed",
            "TH" => "Thu",
            "FR" => "Fri",
            "SA" => "Sat",
            "SU" => "Sun",
            _ => continue,
        };
        names.push(name);
    }
    names.join(", ")
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

#[cfg(test)]
mod tests {
    use super::humanize_rrule;

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
    fn by_day_strips_positional_prefixes() {
        // Android writes "last Friday of the month" as BYDAY=-1FR;
        // we can't render the "last of" nuance cheaply but should at
        // least not choke on the prefix.
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYDAY=-1FR", false),
            "Every month on Fri"
        );
        assert_eq!(
            humanize_rrule("FREQ=MONTHLY;BYDAY=1MO", false),
            "Every month on Mon"
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
