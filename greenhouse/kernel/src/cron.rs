// src/cron.rs — hand-rolled 5-field cron (min hour dom mon dow), no deps.
// Supports * lists a,b ranges a-b steps */n a-b/n a/n. dow 0-7 (7 = Sunday).
// Vixie rule: when BOTH dom and dow are restricted, either may match.

use chrono::{Datelike, Timelike};
use std::collections::HashSet;

const RANGES: [(u32, u32); 5] = [(0, 59), (0, 23), (1, 31), (1, 12), (0, 7)];

#[derive(Debug, Clone)]
pub struct Cron {
    fields: [Option<HashSet<u32>>; 5],
    dom_restricted: bool,
    dow_restricted: bool,
}

fn parse_field(
    field: &str,
    (min, max): (u32, u32),
    is_dow: bool,
) -> Result<Option<HashSet<u32>>, String> {
    if field == "*" {
        return Ok(None);
    }
    let mut out = HashSet::new();
    for part in field.split(',') {
        // forms: N, N-M, N/S, N-M/S, */S
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, Some(s)),
            None => (part, None),
        };
        let step: u32 = match step {
            Some(s) => s
                .parse()
                .map_err(|_| format!("bad cron step in '{field}'"))?,
            None => 1,
        };
        if step < 1 {
            return Err(format!("bad cron step in '{field}'"));
        }
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            let lo = a.parse().map_err(|_| format!("bad cron field '{field}'"))?;
            let hi = b.parse().map_err(|_| format!("bad cron field '{field}'"))?;
            (lo, hi)
        } else {
            let lo: u32 = range
                .parse()
                .map_err(|_| format!("bad cron field '{field}'"))?;
            // 'a/n' runs a..=max (vixie); bare 'a' is just a.
            (
                lo,
                if step > 1 || part.contains('/') {
                    max
                } else {
                    lo
                },
            )
        };
        if lo < min || hi > max || lo > hi {
            return Err(format!("cron value out of range in '{field}'"));
        }
        let mut v = lo;
        while v <= hi {
            out.insert(if is_dow && v == 7 { 0 } else { v });
            let Some(next) = v.checked_add(step) else {
                break;
            };
            v = next;
        }
    }
    Ok(Some(out))
}

pub fn parse_cron(expr: &str) -> Result<Cron, String> {
    let fields: Vec<&str> = expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err(format!(
            "cron '{expr}' must have 5 fields (min hour dom mon dow)"
        ));
    }
    let mut parsed: [Option<HashSet<u32>>; 5] = [None, None, None, None, None];
    for (i, f) in fields.iter().enumerate() {
        parsed[i] = parse_field(f, RANGES[i], i == 4)?;
    }
    Ok(Cron {
        dom_restricted: fields[2] != "*",
        dow_restricted: fields[4] != "*",
        fields: parsed,
    })
}

pub fn cron_matches<T: Timelike + Datelike>(c: &Cron, t: &T) -> bool {
    let has = |i: usize, v: u32| c.fields[i].as_ref().is_none_or(|s| s.contains(&v));
    if !has(0, t.minute()) || !has(1, t.hour()) || !has(3, t.month()) {
        return false;
    }
    let dom_ok = has(2, t.day());
    let dow_ok = has(4, t.weekday().num_days_from_sunday());
    if c.dom_restricted && c.dow_restricted {
        dom_ok || dow_ok
    } else {
        dom_ok && dow_ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn at(day: u32, h: u32, m: u32) -> chrono::NaiveDateTime {
        NaiveDate::from_ymd_opt(2026, 8, day)
            .unwrap()
            .and_hms_opt(h, m, 0)
            .unwrap()
    }

    #[test]
    fn hourly() {
        let c = parse_cron("0 * * * *").unwrap();
        assert!(cron_matches(&c, &at(22, 5, 0)));
        assert!(!cron_matches(&c, &at(22, 5, 1)));
    }

    #[test]
    fn daily_at_4() {
        let c = parse_cron("0 4 * * *").unwrap();
        assert!(cron_matches(&c, &at(22, 4, 0)));
        assert!(!cron_matches(&c, &at(22, 5, 0)));
        assert!(!cron_matches(&c, &at(22, 4, 30)));
    }

    #[test]
    fn steps() {
        let c = parse_cron("*/15 * * * *").unwrap();
        for m in [0, 15, 30, 45] {
            assert!(cron_matches(&c, &at(22, 1, m)));
        }
        assert!(!cron_matches(&c, &at(22, 1, 20)));
    }

    #[test]
    fn range_with_step() {
        let c = parse_cron("10-40/10 * * * *").unwrap();
        for m in [10, 20, 30, 40] {
            assert!(cron_matches(&c, &at(22, 0, m)));
        }
        assert!(!cron_matches(&c, &at(22, 0, 50)));
    }

    #[test]
    fn huge_steps_select_only_the_start() {
        for field in ["1/4294967295", "1-59/4294967295"] {
            let c = parse_cron(&format!("{field} * * * *")).unwrap();
            for minute in 0..60 {
                assert_eq!(cron_matches(&c, &at(22, 0, minute)), minute == 1);
            }
        }
    }

    #[test]
    fn dow_monday() {
        // 2026-08-24 is a Monday, 2026-08-22 a Saturday.
        let c = parse_cron("30 2 * * 1").unwrap();
        assert!(cron_matches(&c, &at(24, 2, 30)));
        assert!(!cron_matches(&c, &at(22, 2, 30)));
    }

    #[test]
    fn dow_seven_is_sunday() {
        let c = parse_cron("0 0 * * 7").unwrap();
        assert!(cron_matches(&c, &at(23, 0, 0))); // 2026-08-23 is a Sunday
    }

    #[test]
    fn vixie_or() {
        let c = parse_cron("0 0 1 * 1").unwrap();
        assert!(cron_matches(&c, &at(24, 0, 0))); // Monday, not the 1st
        let first = NaiveDate::from_ymd_opt(2026, 9, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        assert!(cron_matches(&c, &first)); // the 1st, a Tuesday
        assert!(!cron_matches(&c, &at(22, 0, 0))); // Saturday the 22nd
    }

    #[test]
    fn rejects_malformed() {
        for bad in [
            "61 * * * *",
            "* * * *",
            "a * * * *",
            "*/0 * * * *",
            "5-2 * * * *",
        ] {
            assert!(parse_cron(bad).is_err(), "{bad} should fail");
        }
    }
}
