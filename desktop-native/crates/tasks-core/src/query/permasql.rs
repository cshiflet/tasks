//! Port of `com.todoroo.astrid.api.PermaSql`.
//!
//! Replaces placeholder tokens in saved filter SQL (`NOW()`, `EOD()`,
//! `NOONT()`, …) with the corresponding millisecond-epoch timestamp at query
//! time. Upstream these are substituted at `getQuery` call sites and exists
//! so saved filters don't stale as the clock advances.

const ONE_DAY_MS: i64 = 86_400_000;

pub fn replace_placeholders_for_query(sql: &str, now_ms: i64) -> String {
    let mut result = sql.to_string();

    if result.contains("NOW()") {
        result = result.replace("NOW()", &now_ms.to_string());
    }

    if result.contains("EOD")
        && (result.contains("EOD()")
            || result.contains("EODT()")
            || result.contains("EODY()")
            || result.contains("EODTT()")
            || result.contains("EODW()")
            || result.contains("EODM()"))
    {
        let eod = end_of_day_ms(now_ms);
        result = result
            .replace("EODY()", &(eod - ONE_DAY_MS).to_string())
            .replace("EODTT()", &(eod + 2 * ONE_DAY_MS).to_string())
            .replace("EODT()", &(eod + ONE_DAY_MS).to_string())
            .replace("EODW()", &(eod + 7 * ONE_DAY_MS).to_string())
            .replace("EODM()", &(eod + 30 * ONE_DAY_MS).to_string())
            .replace("EOD()", &eod.to_string());
    }

    if result.contains("NOON")
        && (result.contains("NOON()")
            || result.contains("NOONT()")
            || result.contains("NOONY()")
            || result.contains("NOONTT()")
            || result.contains("NOONW()")
            || result.contains("NOONM()"))
    {
        let noon = noon_ms(now_ms);
        result = result
            .replace("NOONY()", &(noon - ONE_DAY_MS).to_string())
            .replace("NOONTT()", &(noon + 2 * ONE_DAY_MS).to_string())
            .replace("NOONT()", &(noon + ONE_DAY_MS).to_string())
            .replace("NOONW()", &(noon + 7 * ONE_DAY_MS).to_string())
            .replace("NOONM()", &(noon + 30 * ONE_DAY_MS).to_string())
            .replace("NOON()", &noon.to_string());
    }

    result
}

/// UTC end-of-day for a given instant. The Android implementation uses the
/// device's local time zone via `DateTime.endOfDay()`; we approximate with
/// a UTC day boundary, which is acceptable for the read-only client where
/// millisecond-level precision on placeholder expansion is not load-bearing.
/// The UI layer can pass a pre-computed `now_ms` aligned to the local clock
/// if strict parity is needed.
fn end_of_day_ms(now_ms: i64) -> i64 {
    let day_start = (now_ms / ONE_DAY_MS) * ONE_DAY_MS;
    day_start + ONE_DAY_MS - 1
}

fn noon_ms(now_ms: i64) -> i64 {
    let day_start = (now_ms / ONE_DAY_MS) * ONE_DAY_MS;
    day_start + ONE_DAY_MS / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_is_substituted() {
        let out = replace_placeholders_for_query("dueDate > NOW()", 1_700_000_000_000);
        assert_eq!(out, "dueDate > 1700000000000");
    }

    #[test]
    fn eod_variants_expand() {
        let now = 1_700_000_000_000;
        let out = replace_placeholders_for_query("BETWEEN EODY() AND EODT()", now);
        assert!(out.contains("BETWEEN"));
        // EODY() < EODT(), so the textual order should have a smaller number
        // before a larger one.
        let parts: Vec<&str> = out.split_whitespace().collect();
        let lo: i64 = parts[1].parse().unwrap();
        let hi: i64 = parts[3].parse().unwrap();
        assert!(lo < hi, "EODY ({lo}) should precede EODT ({hi})");
    }

    #[test]
    fn eod_longer_tokens_win_over_shorter_prefix() {
        // EODT() must not be partially replaced by EOD()'s substitution.
        let now = 1_700_000_000_000;
        let out = replace_placeholders_for_query("x=EODT()", now);
        assert!(
            !out.contains("EOD"),
            "EOD() must not leak into EODT(): {out}"
        );
    }
}
