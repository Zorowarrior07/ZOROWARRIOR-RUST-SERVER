//! UTC timestamp handling without third-party date crates.
//!
//! ZWRS stores timestamps as ISO-8601 UTC strings (`2026-11-10T10:00:00Z`) for
//! human readability and as Unix seconds for arithmetic. Both representations
//! are lossless to the second, which is the precision sessions need.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch.
pub type UnixSeconds = i64;

/// Formats Unix seconds as `YYYY-MM-DDTHH:MM:SSZ`.
#[must_use]
pub fn format_utc(secs: UnixSeconds) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Parses `YYYY-MM-DDTHH:MM:SSZ` (also accepts a space separator and `+00:00`).
///
/// # Errors
/// Returns a human-readable message describing the first invalid component.
pub fn parse_utc(text: &str) -> Result<UnixSeconds, String> {
    let t = text.trim();
    if t.len() < 19 {
        return Err(format!("timestamp too short: {text:?}"));
    }
    let bytes = t.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || (bytes[10] != b'T' && bytes[10] != b' ') {
        return Err(format!("timestamp layout must be YYYY-MM-DDTHH:MM:SSZ, got {text:?}"));
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return Err(format!("timestamp time must be HH:MM:SS, got {text:?}"));
    }
    let num = |start: usize, end: usize| -> Result<i64, String> {
        let slice = t
            .get(start..end)
            .ok_or_else(|| format!("timestamp truncated: {text:?}"))?;
        slice
            .parse::<i64>()
            .map_err(|_| format!("invalid number {slice:?} in {text:?}"))
    };
    let year = num(0, 4)?;
    let month = num(5, 7)?;
    let day = num(8, 10)?;
    let hour = num(11, 13)?;
    let minute = num(14, 16)?;
    let second = num(17, 19)?;

    if !(1..=12).contains(&month) {
        return Err(format!("month out of range: {month}"));
    }
    if !(1..=31).contains(&day) {
        return Err(format!("day out of range: {day}"));
    }
    if hour > 23 || minute > 59 || second > 60 {
        return Err(format!("time out of range in {text:?}"));
    }
    if month == 2 {
        let limit = if is_leap_year(year) { 29 } else { 28 };
        if day > limit {
            return Err(format!("february {year} has at most {limit} days"));
        }
    }
    if matches!(month, 4 | 6 | 9 | 11) && day > 30 {
        return Err(format!("month {month} has at most 30 days"));
    }

    let rest = &t[19..];
    match rest {
        "Z" | "z" | "" => {}
        "+00:00" | "+0000" => {}
        other => return Err(format!("unsupported timezone suffix {other:?}")),
    }

    Ok(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second)
}

/// Current wall-clock time in Unix seconds.
///
/// Returns `0` if the system clock is before the Unix epoch, which only happens
/// on badly misconfigured hosts; the engine logs this condition at startup.
#[must_use]
pub fn now_unix() -> UnixSeconds {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// Whether `year` is a leap year in the proleptic Gregorian calendar.
#[must_use]
pub fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// Howard Hinnant's civil calendar algorithms (public domain), proleptic
// Gregorian, valid for the ranges ZWRS needs.

/// Days since 1970-01-01 for a civil date.
#[must_use]
pub fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date for a count of days since 1970-01-01.
#[must_use]
pub fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A point in time as both Unix seconds and its ISO-8601 rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(UnixSeconds);

impl Timestamp {
    /// Wraps Unix seconds.
    #[must_use]
    pub const fn from_unix(secs: UnixSeconds) -> Self {
        Self(secs)
    }

    /// Current time.
    #[must_use]
    pub fn now() -> Self {
        Self(now_unix())
    }

    /// Parses an ISO-8601 UTC timestamp.
    ///
    /// # Errors
    /// Propagates the parse failure message.
    pub fn parse(text: &str) -> Result<Self, String> {
        parse_utc(text).map(Self)
    }

    /// Unix seconds.
    #[must_use]
    pub const fn unix(&self) -> UnixSeconds {
        self.0
    }

    /// Seconds elapsed until `later`, saturating at zero so that clock skew can
    /// never produce negative playtime.
    #[must_use]
    pub fn saturating_until(&self, later: Timestamp) -> u64 {
        u64::try_from(later.0 - self.0).unwrap_or(0)
    }
}

impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&format_utc(self.0))
    }
}

/// Injectable clock so session/playtime logic is testable without sleeping.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Current time.
    fn now(&self) -> Timestamp;
}

/// Wall-clock implementation.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_and_known_instants() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(1_700_000_000), "2023-11-14T22:13:20Z");
        assert_eq!(format_utc(1_763_000_000), "2025-11-13T02:13:20Z");
        assert_eq!(format_utc(-1), "1969-12-31T23:59:59Z");
    }

    #[test]
    fn roundtrip_many_instants() {
        let mut secs = -100_000_000i64;
        while secs < 4_000_000_000 {
            let text = format_utc(secs);
            assert_eq!(parse_utc(&text), Ok(secs), "roundtrip failed for {text}");
            secs += 86_407;
        }
    }

    #[test]
    fn civil_days_roundtrip() {
        for days in [-1000, 0, 1, 19_000, 25_000, 100_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }

    #[test]
    fn leap_years() {
        assert_eq!(parse_utc("2024-02-29T00:00:00Z").unwrap(), 1_709_164_800);
        assert_eq!(parse_utc("2000-02-29T00:00:00Z").is_ok(), true, "2000 is a leap year");
        assert!(parse_utc("1900-02-29T00:00:00Z").is_err(), "1900 is not");
        assert!(parse_utc("2023-02-29T00:00:00Z").is_err());
        assert!(parse_utc("2023-04-31T00:00:00Z").is_err());
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "",
            "2023-11-14",
            "2023-11-14T22:13",
            "2023/11/14T22:13:20Z",
            "2023-11-14T25:13:20Z",
            "2023-13-14T22:13:20Z",
            "2023-11-14T22:13:20+05:30",
            "xxxx-11-14T22:13:20Z",
        ] {
            assert!(parse_utc(bad).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn playtime_never_negative() {
        let a = Timestamp::from_unix(100);
        let b = Timestamp::from_unix(50);
        assert_eq!(a.saturating_until(b), 0);
        assert_eq!(b.saturating_until(a), 50);
        assert_eq!(a.to_string(), "1970-01-01T00:01:40Z");
    }
}
