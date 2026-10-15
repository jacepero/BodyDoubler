// src/clock.rs — wall-clock time in a person's time zone, for the assistant's
// clock, timers and reminders (timers.rs).
//
// Time zones come from the host's zone database (/usr/share/zoneinfo, TZif
// files) rather than a library: the table of past transitions, then the
// POSIX rule in the file's footer (for example "EST5EDT,M3.2.0,M11.1.0") for
// everything after it. A reminder set before a daylight-saving change still
// lands at the wall time the person asked for.

use std::path::PathBuf;

const ZONEINFO: &str = "/usr/share/zoneinfo";

// ---- civil dates ---------------------------------------------------------------

/// Days since 1970-01-01 of a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day count since 1970-01-01.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = Sunday.
pub fn weekday(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

pub const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

fn leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_days(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if leap(y) => 29,
        _ => 28,
    }
}

// ---- POSIX TZ rules --------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
enum RuleDate {
    /// Mm.w.d: day d (0 = Sunday) of week w (5 = last) of month m.
    Month(u32, u32, u32),
    /// Jn: day n of the year, 1..=365, never counting February 29.
    Julian1(u32),
    /// n: day n of the year, 0..=365, counting February 29.
    Julian0(u32),
}

#[derive(Clone, Debug, PartialEq)]
struct Rule {
    date: RuleDate,
    /// Seconds after local midnight (may be negative or beyond a day).
    time: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct PosixTz {
    std_abbr: String,
    /// Seconds east of UTC (the POSIX string writes them west).
    std_offset: i64,
    dst: Option<(String, i64, Rule, Rule)>,
}

fn parse_abbr(s: &str) -> Option<(String, &str)> {
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>')?;
        return Some((rest[..end].to_string(), &rest[end + 1..]));
    }
    let end = s
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    (end >= 3).then(|| (s[..end].to_string(), &s[end..]))
}

/// `[+-]hh[:mm[:ss]]` as seconds, with the sign as written.
fn parse_hms(s: &str) -> Option<(i64, &str)> {
    let (sign, s) = match s.as_bytes().first() {
        Some(b'-') => (-1, &s[1..]),
        Some(b'+') => (1, &s[1..]),
        _ => (1, s),
    };
    let end = s
        .find(|c: char| !(c.is_ascii_digit() || c == ':'))
        .unwrap_or(s.len());
    let mut parts = s[..end].split(':').map(|p| p.parse::<i64>().ok());
    let h = parts.next()??;
    let m = parts.next().unwrap_or(Some(0))?;
    let sec = parts.next().unwrap_or(Some(0))?;
    Some((sign * (h * 3600 + m * 60 + sec), &s[end..]))
}

fn parse_rule(s: &str) -> Option<(Rule, &str)> {
    let (date, rest) = if let Some(r) = s.strip_prefix('M') {
        let end = r
            .find(|c: char| !(c.is_ascii_digit() || c == '.'))
            .unwrap_or(r.len());
        let n: Vec<u32> = r[..end]
            .split('.')
            .map(|p| p.parse().ok())
            .collect::<Option<_>>()?;
        if n.len() != 3 || !(1..=12).contains(&n[0]) || !(1..=5).contains(&n[1]) || n[2] > 6 {
            return None;
        }
        (RuleDate::Month(n[0], n[1], n[2]), &r[end..])
    } else if let Some(r) = s.strip_prefix('J') {
        let end = r.find(|c: char| !c.is_ascii_digit()).unwrap_or(r.len());
        (RuleDate::Julian1(r[..end].parse().ok()?), &r[end..])
    } else {
        let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
        (RuleDate::Julian0(s[..end].parse().ok()?), &s[end..])
    };
    let (time, rest) = match rest.strip_prefix('/') {
        Some(t) => parse_hms(t)?,
        None => (7200, rest),
    };
    Some((Rule { date, time }, rest))
}

impl PosixTz {
    fn parse(s: &str) -> Option<PosixTz> {
        let (std_abbr, rest) = parse_abbr(s)?;
        let (west, rest) = parse_hms(rest)?;
        let std_offset = -west;
        if rest.is_empty() {
            return Some(PosixTz {
                std_abbr,
                std_offset,
                dst: None,
            });
        }
        let (dst_abbr, rest) = parse_abbr(rest)?;
        let (dst_offset, rest) = if rest.starts_with(',') {
            (std_offset + 3600, rest)
        } else {
            let (west, rest) = parse_hms(rest)?;
            (-west, rest)
        };
        let rest = rest.strip_prefix(',')?;
        let (start, rest) = parse_rule(rest)?;
        let rest = rest.strip_prefix(',')?;
        let (end, rest) = parse_rule(rest)?;
        rest.is_empty().then_some(PosixTz {
            std_abbr,
            std_offset,
            dst: Some((dst_abbr, dst_offset, start, end)),
        })
    }

    /// Local seconds since the epoch at which the rule's transition happens.
    fn rule_local(rule: &Rule, year: i64) -> i64 {
        let day = match rule.date {
            RuleDate::Month(m, w, d) => {
                let first = days_from_civil(year, m, 1);
                let offset = (d + 7 - weekday(first)) % 7;
                let mut day = 1 + offset + (w - 1) * 7;
                if day > month_days(year, m) {
                    day -= 7;
                }
                days_from_civil(year, m, day)
            }
            RuleDate::Julian1(n) => {
                let jan1 = days_from_civil(year, 1, 1);
                let n = n as i64 - 1;
                jan1 + n + i64::from(leap(year) && n >= 59)
            }
            RuleDate::Julian0(n) => days_from_civil(year, 1, 1) + n as i64,
        };
        day * 86_400 + rule.time
    }

    /// (offset east of UTC, abbreviation) at a UTC instant.
    fn at(&self, utc: i64) -> (i64, String) {
        let Some((dst_abbr, dst_offset, start, end)) = &self.dst else {
            return (self.std_offset, self.std_abbr.clone());
        };
        let (year, _, _) = civil_from_days((utc + self.std_offset).div_euclid(86_400));
        // Start is written in standard time, end in daylight time.
        let starts = Self::rule_local(start, year) - self.std_offset;
        let ends = Self::rule_local(end, year) - dst_offset;
        let in_dst = if starts < ends {
            utc >= starts && utc < ends
        } else {
            !(utc >= ends && utc < starts)
        };
        if in_dst {
            (*dst_offset, dst_abbr.clone())
        } else {
            (self.std_offset, self.std_abbr.clone())
        }
    }
}

// ---- zones ----------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Zone {
    pub name: String,
    transitions: Vec<i64>,
    /// The local-time type in force from each transition.
    kinds: Vec<usize>,
    /// (offset east of UTC, abbreviation).
    types: Vec<(i64, String)>,
    footer: Option<PosixTz>,
}

/// An IANA zone name as it may appear in a request: letters, digits and
/// `_ + - /`, no empty or dot segments.
pub fn valid_zone_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.split('/').all(|part| {
            !part.is_empty()
                && !part.starts_with('.')
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-'))
        })
}

fn be_i32(b: &[u8]) -> i64 {
    i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as i64
}

fn be_i64(b: &[u8]) -> i64 {
    i64::from_be_bytes(b[..8].try_into().unwrap())
}

impl Zone {
    pub fn utc() -> Zone {
        Zone {
            name: "UTC".into(),
            transitions: vec![],
            kinds: vec![],
            types: vec![(0, "UTC".into())],
            footer: None,
        }
    }

    /// The zone from the host's zone database.
    pub fn load(name: &str) -> Result<Zone, String> {
        if name == "UTC" || name == "Etc/UTC" {
            return Ok(Zone::utc());
        }
        if !valid_zone_name(name) {
            return Err(format!("'{name}' is not a time zone name"));
        }
        let dir = std::env::var("KERNEL_ZONEINFO").unwrap_or_else(|_| ZONEINFO.to_string());
        let bytes = std::fs::read(PathBuf::from(dir).join(name))
            .map_err(|_| format!("unknown time zone '{name}'"))?;
        Zone::parse(name, &bytes)
            .ok_or_else(|| format!("the time zone file for '{name}' is not readable"))
    }

    /// A TZif file (RFC 8536), version 1 or later.
    pub fn parse(name: &str, bytes: &[u8]) -> Option<Zone> {
        let header = |at: usize| -> Option<[usize; 6]> {
            let h = bytes.get(at..at + 44)?;
            if &h[..4] != b"TZif" {
                return None;
            }
            let mut counts = [0usize; 6];
            for (i, c) in counts.iter_mut().enumerate() {
                *c = be_i32(&h[20 + i * 4..]) as usize;
            }
            Some(counts)
        };
        let version = *bytes.get(4)?;
        let [isutc, isstd, leaps, times, types, chars] = header(0)?;
        let v1_len = times * 5 + types * 6 + chars + leaps * 8 + isstd + isutc;
        let (at, width, counts) = if version >= b'2' {
            let at = 44 + v1_len;
            (at + 44, 8, header(at)?)
        } else {
            (44, 4, [isutc, isstd, leaps, times, types, chars])
        };
        let [isutc, isstd, leaps, times, types, chars] = counts;
        let mut p = at;
        let take = |p: &mut usize, n: usize| -> Option<&[u8]> {
            let slice = bytes.get(*p..*p + n)?;
            *p += n;
            Some(slice)
        };
        let raw_times = take(&mut p, times * width)?;
        let transitions: Vec<i64> = raw_times
            .chunks(width)
            .map(|c| if width == 8 { be_i64(c) } else { be_i32(c) })
            .collect();
        let kinds: Vec<usize> = take(&mut p, times)?.iter().map(|&k| k as usize).collect();
        let raw_types = take(&mut p, types * 6)?.to_vec();
        let abbrs = take(&mut p, chars)?.to_vec();
        take(&mut p, leaps * (width + 4) + isstd + isutc)?;
        let types: Vec<(i64, String)> = raw_types
            .chunks(6)
            .map(|t| {
                let start = t[5] as usize;
                let end = abbrs[start..]
                    .iter()
                    .position(|&b| b == 0)
                    .map(|e| start + e)
                    .unwrap_or(abbrs.len());
                (
                    be_i32(t),
                    String::from_utf8_lossy(&abbrs[start..end]).to_string(),
                )
            })
            .collect();
        if types.is_empty() || kinds.iter().any(|&k| k >= types.len()) {
            return None;
        }
        let footer = if version >= b'2' {
            let rest = bytes.get(p..).unwrap_or(&[]);
            let text = String::from_utf8_lossy(rest);
            let line = text
                .trim_matches('\n')
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            (!line.is_empty()).then(|| PosixTz::parse(&line)).flatten()
        } else {
            None
        };
        Some(Zone {
            name: name.to_string(),
            transitions,
            kinds,
            types,
            footer,
        })
    }

    /// (offset east of UTC in seconds, abbreviation) at a UTC instant.
    pub fn offset_at(&self, utc: i64) -> (i64, String) {
        match self.transitions.last() {
            Some(&last) if utc < last || self.footer.is_none() => {
                let i = self.transitions.partition_point(|&t| t <= utc);
                if i == 0 {
                    // Before the first transition: the first standard-time type.
                    self.types[0].clone()
                } else {
                    self.types[self.kinds[i - 1]].clone()
                }
            }
            _ => match &self.footer {
                Some(tz) => tz.at(utc),
                None => self.types[0].clone(),
            },
        }
    }

    /// The UTC instant of a local wall time. A time skipped by a
    /// daylight-saving change counts as the time after it; a repeated one as
    /// its first occurrence.
    pub fn local_to_utc(&self, local: i64) -> i64 {
        let before = self.offset_at(local - 86_400).0;
        let after = self.offset_at(local + 86_400).0;
        let first = local - before;
        if self.offset_at(first).0 == before {
            return first;
        }
        let second = local - after;
        if self.offset_at(second).0 == after {
            return second;
        }
        // In a gap: move forward by the change.
        local - before
    }
}

/// A local wall time, written for people and models:
/// "Friday 2026-09-25 21:04 EDT (UTC-04:00)".
pub fn describe(zone: &Zone, utc: i64) -> String {
    let (offset, abbr) = zone.offset_at(utc);
    let local = utc + offset;
    let days = local.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    let secs = local.rem_euclid(86_400);
    let sign = if offset < 0 { '-' } else { '+' };
    format!(
        "{} {y:04}-{m:02}-{d:02} {:02}:{:02} {abbr} (UTC{sign}{:02}:{:02})",
        WEEKDAYS[weekday(days) as usize],
        secs / 3600,
        (secs % 3600) / 60,
        offset.abs() / 3600,
        (offset.abs() % 3600) / 60,
    )
}

/// "YYYY-MM-DD HH:MM[:SS]" (or with a "T") as local seconds since the epoch.
pub fn parse_local(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, time) = text.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let m: u32 = d.next()?.parse().ok()?;
    let day: u32 = d.next()?.parse().ok()?;
    if d.next().is_some() || !(1..=12).contains(&m) || day == 0 || day > month_days(y, m) {
        return None;
    }
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let s: i64 = t.next().map(|s| s.parse().ok()).unwrap_or(Some(0))?;
    if t.next().is_some() || h > 23 || min > 59 || s > 59 {
        return None;
    }
    Some(days_from_civil(y, m, day) * 86_400 + h * 3600 + min * 60 + s)
}

/// "HH:MM" as seconds after midnight.
pub fn parse_clock(text: &str) -> Option<i64> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 3600 + m * 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toronto_rule() -> Zone {
        Zone {
            name: "America/Toronto".into(),
            transitions: vec![],
            kinds: vec![],
            types: vec![(-5 * 3600, "EST".into())],
            footer: PosixTz::parse("EST5EDT,M3.2.0,M11.1.0"),
        }
    }

    fn utc(y: i64, m: u32, d: u32, h: i64, min: i64) -> i64 {
        days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60
    }

    #[test]
    fn civil_dates_round_trip() {
        for days in [-719_468, -1, 0, 11_016, 20_721, 50_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(days_from_civil(2026, 9, 25)), (2026, 9, 25));
        assert_eq!(
            WEEKDAYS[weekday(days_from_civil(2026, 9, 25)) as usize],
            "Friday"
        );
    }

    #[test]
    fn posix_rules_parse() {
        assert_eq!(
            PosixTz::parse("EST5EDT,M3.2.0,M11.1.0")
                .unwrap()
                .dst
                .unwrap()
                .2,
            Rule {
                date: RuleDate::Month(3, 2, 0),
                time: 7200
            }
        );
        let ist = PosixTz::parse("IST-5:30").unwrap();
        assert_eq!((ist.std_offset, ist.dst.is_none()), (5 * 3600 + 1800, true));
        let chile = PosixTz::parse("<-04>4<-03>,M9.1.6/24,M4.1.6/24").unwrap();
        assert_eq!(chile.std_abbr, "-04");
        assert!(PosixTz::parse("EST5EDT,M13.1.0,M11.1.0").is_none());
    }

    #[test]
    fn daylight_saving_follows_the_rule() {
        let z = toronto_rule();
        // 2026: EDT from Sunday 8 March 02:00 EST to Sunday 1 November 02:00 EDT.
        assert_eq!(z.offset_at(utc(2026, 3, 8, 6, 59)).1, "EST");
        assert_eq!(z.offset_at(utc(2026, 3, 8, 7, 0)).1, "EDT");
        assert_eq!(
            z.offset_at(utc(2026, 9, 26, 1, 55)),
            (-4 * 3600, "EDT".into())
        );
        assert_eq!(z.offset_at(utc(2026, 11, 1, 5, 59)).1, "EDT");
        assert_eq!(z.offset_at(utc(2026, 11, 1, 6, 0)).1, "EST");
        assert_eq!(
            describe(&z, utc(2026, 9, 26, 1, 55)),
            "Friday 2026-09-25 21:55 EDT (UTC-04:00)"
        );
    }

    #[test]
    fn a_wall_time_after_a_change_keeps_its_wall_time() {
        let z = toronto_rule();
        // Set in September for 5 pm on 2 November: after the change, so UTC-5.
        let local = parse_local("2026-11-02 17:00").unwrap();
        assert_eq!(z.local_to_utc(local), utc(2026, 11, 2, 22, 0));
        // 02:30 on 8 March does not exist; it becomes 03:30 EDT.
        assert_eq!(
            z.local_to_utc(parse_local("2026-03-08T02:30").unwrap()),
            utc(2026, 3, 8, 7, 30)
        );
        // 01:30 on 1 November happens twice; the first (EDT) is used.
        assert_eq!(
            z.local_to_utc(parse_local("2026-11-01 01:30").unwrap()),
            utc(2026, 11, 1, 5, 30)
        );
    }

    #[test]
    fn southern_and_offset_only_zones() {
        let sydney = Zone {
            footer: PosixTz::parse("AEST-10AEDT,M10.1.0,M4.1.0/3"),
            ..toronto_rule()
        };
        assert_eq!(sydney.offset_at(utc(2026, 1, 15, 0, 0)).1, "AEDT");
        assert_eq!(sydney.offset_at(utc(2026, 7, 15, 0, 0)).1, "AEST");
        assert_eq!(Zone::utc().offset_at(0), (0, "UTC".into()));
    }

    #[test]
    fn local_times_parse_strictly() {
        assert!(parse_local("2026-02-29 10:00").is_none());
        assert!(parse_local("2028-02-29 10:00").is_some());
        assert!(parse_local("2026-09-25 24:00").is_none());
        assert!(parse_local("2026-09-25").is_none());
        assert_eq!(parse_clock("17:05"), Some(17 * 3600 + 300));
        assert!(parse_clock("5pm").is_none());
        assert!(valid_zone_name("America/Argentina/Buenos_Aires"));
        assert!(valid_zone_name("Etc/GMT+5"));
        assert!(!valid_zone_name("../etc/passwd"));
        assert!(!valid_zone_name("America//Toronto"));
    }

    /// A real TZif file, when the host has one (Linux; skipped elsewhere).
    #[test]
    fn host_zone_files_read() {
        let Ok(z) = Zone::load("America/Toronto") else {
            return;
        };
        assert_eq!(z.offset_at(utc(2026, 9, 26, 1, 55)).1, "EDT");
        assert_eq!(z.offset_at(utc(2026, 12, 1, 0, 0)).1, "EST");
        assert_eq!(z.offset_at(utc(1990, 7, 1, 0, 0)).1, "EDT");
        assert_eq!(z.offset_at(utc(2040, 7, 1, 0, 0)).1, "EDT");
    }
}
