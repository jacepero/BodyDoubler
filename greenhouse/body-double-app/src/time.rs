// Local time for people: labels in their zone, days, and spans. Zones come
// from the kernel crate's clock (the host's zone database).

use greenhouse_kernel::clock::{self, Zone};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The zone a person named, or the fallback when it cannot be loaded.
pub fn zone_or(name: &str, fallback: &Zone) -> Zone {
    Zone::load(name).unwrap_or_else(|_| fallback.clone())
}

/// Seconds since the epoch on the local wall clock.
pub fn local(zone: &Zone, utc: i64) -> i64 {
    utc + zone.offset_at(utc).0
}

/// The local day (days since 1970-01-01) an instant falls on.
pub fn day_of(zone: &Zone, utc: i64) -> i64 {
    local(zone, utc).div_euclid(86_400)
}

/// Minutes after local midnight.
pub fn minute_of_day(zone: &Zone, utc: i64) -> u32 {
    (local(zone, utc).rem_euclid(86_400) / 60) as u32
}

/// The UTC instant a local day starts.
pub fn day_start(zone: &Zone, day: i64) -> i64 {
    zone.local_to_utc(day * 86_400)
}

/// "14:32".
pub fn hhmm(zone: &Zone, utc: i64) -> String {
    let secs = local(zone, utc).rem_euclid(86_400);
    format!("{:02}:{:02}", secs / 3600, (secs % 3600) / 60)
}

/// "Thu 15 Oct".
pub fn date_label(day: i64) -> String {
    let (_, m, d) = clock::civil_from_days(day);
    format!(
        "{} {d} {}",
        &clock::WEEKDAYS[clock::weekday(day) as usize][..3],
        MONTHS[(m - 1) as usize]
    )
}

/// "Thu 15 Oct 14:32".
pub fn stamp(zone: &Zone, utc: i64) -> String {
    format!("{} {}", date_label(day_of(zone, utc)), hhmm(zone, utc))
}

/// "14:32" on the same local day as `now`, else "Thu 15 Oct 14:32".
pub fn when(zone: &Zone, utc: i64, now: i64) -> String {
    if day_of(zone, utc) == day_of(zone, now) {
        hhmm(zone, utc)
    } else {
        stamp(zone, utc)
    }
}

/// "2026-10-15".
pub fn iso_date(day: i64) -> String {
    let (y, m, d) = clock::civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A "YYYY-MM-DD" date as a day count.
pub fn parse_date(text: &str) -> Option<i64> {
    let day = clock::parse_local(&format!("{} 00:00", text.trim()))?.div_euclid(86_400);
    // parse_local is lenient about digits; insist on the exact form.
    (iso_date(day) == text.trim()).then_some(day)
}

/// "2026-10-15T18:32:00Z".
pub fn iso_utc(utc: i64) -> String {
    let (y, m, d) = clock::civil_from_days(utc.div_euclid(86_400));
    let secs = utc.rem_euclid(86_400);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

/// "HH:MM" as minutes after midnight.
pub fn parse_hhmm(text: &str) -> Option<u32> {
    clock::parse_clock(text).map(|secs| (secs / 60) as u32)
}

/// Minutes after midnight as "HH:MM".
pub fn fmt_hhmm(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

/// How long, the way a person says it: "0m", "45m", "2h", "2h 10m", "3d 4h".
pub fn span(minutes: i64) -> String {
    let minutes = minutes.max(0);
    if minutes < 60 {
        return format!("{minutes}m");
    }
    if minutes < 48 * 60 {
        let (h, m) = (minutes / 60, minutes % 60);
        return if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m}m")
        };
    }
    let (d, h) = (minutes / 1440, (minutes % 1440) / 60);
    if h == 0 {
        format!("{d}d")
    } else {
        format!("{d}d {h}h")
    }
}

/// Whole minutes from `from` to `to`, never negative.
pub fn minutes_between(from: i64, to: i64) -> i64 {
    ((to - from) / 60).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_read_the_way_people_write_them() {
        let utc = Zone::utc();
        let t = clock::days_from_civil(2026, 10, 15) * 86_400 + 14 * 3600 + 32 * 60;
        assert_eq!(hhmm(&utc, t), "14:32");
        assert_eq!(stamp(&utc, t), "Thu 15 Oct 14:32");
        assert_eq!(when(&utc, t, t + 3600), "14:32");
        assert_eq!(when(&utc, t, t + 86_400), "Thu 15 Oct 14:32");
        assert_eq!(iso_utc(t), "2026-10-15T14:32:00Z");
        assert_eq!(iso_date(day_of(&utc, t)), "2026-10-15");
    }

    #[test]
    fn dates_and_clock_times_parse_strictly() {
        assert_eq!(
            parse_date("2026-10-15"),
            Some(clock::days_from_civil(2026, 10, 15))
        );
        assert_eq!(parse_date("2026-02-30"), None);
        assert_eq!(parse_date("15/10/2026"), None);
        assert_eq!(parse_date("2026-1-5"), None);
        assert_eq!(parse_hhmm("22:00"), Some(1320));
        assert_eq!(parse_hhmm("08:30"), Some(510));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(fmt_hhmm(510), "08:30");
    }

    #[test]
    fn spans_are_short() {
        assert_eq!(span(0), "0m");
        assert_eq!(span(-5), "0m");
        assert_eq!(span(45), "45m");
        assert_eq!(span(120), "2h");
        assert_eq!(span(130), "2h 10m");
        assert_eq!(span(3 * 1440 + 4 * 60 + 5), "3d 4h");
        assert_eq!(span(2 * 1440), "2d");
    }

    #[test]
    fn local_days_follow_the_zone() {
        let Ok(ny) = Zone::load("America/New_York") else {
            return; // no zone database on this host
        };
        // 2026-10-16 02:00 UTC is still the 15th in New York (EDT, UTC-4).
        let t = clock::days_from_civil(2026, 10, 16) * 86_400 + 2 * 3600;
        assert_eq!(iso_date(day_of(&ny, t)), "2026-10-15");
        assert_eq!(hhmm(&ny, t), "22:00");
        assert_eq!(minute_of_day(&ny, t), 22 * 60);
        let start = day_start(&ny, day_of(&ny, t));
        assert_eq!(iso_utc(start), "2026-10-15T04:00:00Z");
    }
}
