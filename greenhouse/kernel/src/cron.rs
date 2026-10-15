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
        let num = |s: &str| {
            s.parse::<u32>()
                .map_err(|_| format!("bad cron value '{s}' in '{field}'"))
        };
        let (lo, hi) = if range == "*" {
            (min, max)
        } else if let Some((a, b)) = range.split_once('-') {
            (num(a)?, num(b)?)
        } else {
            let n = num(range)?;
            (
                n,
                // Stand-in, lines 41-63 not captured: N/S runs to the field's end.
                if step > 1 || part.contains('/') {
                    max
                } else {
                    n
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

    // Stand-in: the kernel's tests were captured only in part. `at`, `steps`
    // and `range_with_step` are reconstructed from their names and call
    // sites; `huge_steps_select_only_the_start` is verbatim (PR #29).

    /// A minute in August 2026 (the 22nd is a Saturday, the 24th a Monday).
    fn at(day: u32, hour: u32, minute: u32) -> chrono::NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(2026, 8, day)
            .unwrap()
            .and_hms_opt(hour, minute, 0)
            .unwrap()
    }

    #[test]
    fn every_minute() {
        let c = parse_cron("* * * * *").unwrap();
        for minute in 0..60 {
            assert!(cron_matches(&c, &at(22, 13, minute)));
        }
    }

    #[test]
    fn steps() {
        let c = parse_cron("*/10 * * * *").unwrap();
        for minute in 0..60 {
            assert_eq!(cron_matches(&c, &at(22, 0, minute)), minute % 10 == 0);
        }
        let c = parse_cron("5/20 * * * *").unwrap();
        for minute in 0..60 {
            assert_eq!(
                cron_matches(&c, &at(22, 0, minute)),
                [5, 25, 45].contains(&minute)
            );
        }
    }

    #[test]
    fn range_with_step() {
        let c = parse_cron("10-30/10 22 * * *").unwrap();
        assert!(cron_matches(&c, &at(22, 22, 10)));
        assert!(cron_matches(&c, &at(22, 22, 20)));
        assert!(cron_matches(&c, &at(22, 22, 30)));
        assert!(!cron_matches(&c, &at(22, 22, 40)));
        assert!(!cron_matches(&c, &at(22, 21, 10)));
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
        let c = parse_cron("0 9 * * 1").unwrap();
        assert!(cron_matches(&c, &at(24, 9, 0)));
        assert!(!cron_matches(&c, &at(22, 9, 0)));
        // 7 is Sunday, as 0 is.
        let sunday = parse_cron("0 9 * * 7").unwrap();
        assert!(cron_matches(&sunday, &at(23, 9, 0)));
    }

    #[test]
    fn a_fixed_time_of_day() {
        let c = parse_cron("30 3 * * *").unwrap();
        assert!(cron_matches(&c, &at(22, 3, 30)));
        assert!(cron_matches(&c, &at(24, 3, 30)));
        assert!(!cron_matches(&c, &at(22, 3, 31)));
        assert!(!cron_matches(&c, &at(22, 15, 30)));
    }

    #[test]
    fn dom_or_dow_when_both_are_restricted() {
        // The 1st of the month or any Monday.
        let c = parse_cron("0 0 1 * 1").unwrap();
        assert!(cron_matches(&c, &at(1, 0, 0)));
        assert!(cron_matches(&c, &at(24, 0, 0)));
        assert!(!cron_matches(&c, &at(22, 0, 0)));
    }

    #[test]
    fn bad_expressions_are_refused() {
        assert!(parse_cron("99 * * * *")
            .unwrap_err()
            .contains("out of range"));
        assert!(parse_cron("* 24 * * *").is_err());
        assert!(parse_cron("* * 0 * *").is_err());
        assert!(parse_cron("* * * 13 *").is_err());
        assert!(parse_cron("* * * * 8").is_err());
        assert!(parse_cron("30-10 * * * *").is_err());
        assert!(parse_cron("*/0 * * * *").unwrap_err().contains("step"));
        assert!(parse_cron("x * * * *").is_err());
        assert!(parse_cron("@daily").unwrap_err().contains("5 fields"));
        assert!(parse_cron("* * * *").is_err());
        assert!(parse_cron("* * * * * *").is_err());
    }
}
