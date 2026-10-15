// Check-ins: when a person who has gone quiet hears from their body double.
//
// iOS gave the July app no background process, so it scheduled a chain of
// the next eight notifications and rebuilt the chain whenever the person was
// active. Greenhouse calls checkin_tick every minute instead (an approved
// manifest schedule), and the plan below decides who is due. The rules are
// the same: activity restarts the clock, quiet hours move a check-in to their
// end, and eight unanswered in a row stop the chain until the person is back.

use crate::config::PlanDefaults;
use crate::time;
use greenhouse_kernel::clock::Zone;

/// The iOS chain length: this many unanswered check-ins in a row, then quiet
/// until the person is next active.
pub const MAX_UNANSWERED: i64 = 8;

/// One person's check-in plan.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub user_id: String,
    pub enabled: bool,
    pub every_minutes: i64,
    /// Quiet hours as minutes after local midnight: (from, until). The window
    /// may wrap midnight.
    pub quiet: Option<(u32, u32)>,
    pub timezone: String,
    /// When the next check-in of the chain is due; None when nothing is
    /// scheduled.
    pub next_due_at: Option<i64>,
    /// A check-in the person asked for at a set time ("check on me in 25
    /// minutes"). Like the iOS one-off notification it is separate from the
    /// chain: activity does not move it, quiet hours do not hold it, and it
    /// comes even when the chain is off.
    pub oneoff_at: Option<i64>,
    /// Check-ins raised since the person was last active.
    pub unanswered: i64,
}

impl Plan {
    pub fn new(user_id: &str, defaults: &PlanDefaults, timezone: &str) -> Plan {
        Plan {
            user_id: user_id.into(),
            enabled: defaults.on,
            every_minutes: defaults.every_minutes,
            quiet: defaults.quiet,
            timezone: timezone.into(),
            next_due_at: None,
            oneoff_at: None,
            unanswered: 0,
        }
    }

    /// The next check-in of either kind, if any.
    pub fn next(&self) -> Option<i64> {
        let chain = self.next_due_at.filter(|_| self.enabled);
        match (chain, self.oneoff_at) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// Whether a minute of the day falls in the quiet window [from, until).
pub fn in_quiet(quiet: Option<(u32, u32)>, minute: u32) -> bool {
    match quiet {
        None => false,
        Some((from, until)) if from == until => false,
        Some((from, until)) if from < until => minute >= from && minute < until,
        Some((from, until)) => minute >= from || minute < until,
    }
}

/// The first end of quiet hours after `utc`.
pub fn quiet_end_after(zone: &Zone, until: u32, utc: i64) -> i64 {
    let day = time::day_of(zone, utc);
    for offset in 0..=2 {
        let end = zone.local_to_utc((day + offset) * 86_400 + i64::from(until) * 60);
        if end > utc {
            return end;
        }
    }
    utc + 86_400
}

/// `utc`, or the end of quiet hours when `utc` falls inside them.
pub fn outside_quiet(zone: &Zone, quiet: Option<(u32, u32)>, utc: i64) -> i64 {
    match quiet {
        Some((_, until)) if in_quiet(quiet, time::minute_of_day(zone, utc)) => {
            quiet_end_after(zone, until, utc)
        }
        _ => utc,
    }
}

/// The check-in after one at (or activity at) `utc`.
pub fn next_after(plan: &Plan, zone: &Zone, utc: i64) -> i64 {
    outside_quiet(zone, plan.quiet, utc + plan.every_minutes * 60)
}

/// The person was active at `now`: the count restarts and so does the clock.
pub fn after_activity(plan: &mut Plan, zone: &Zone, now: i64) {
    plan.unanswered = 0;
    plan.next_due_at = plan.enabled.then(|| next_after(plan, zone, now));
}

/// What the tick does with a plan.
#[derive(Clone, Debug, PartialEq)]
pub enum Step {
    /// Nothing is due.
    Wait,
    /// Inside quiet hours: nothing is raised; the chain's check-in moves to
    /// their end.
    Defer { until: i64 },
    /// Raise a check-in. The chain's next one is then due at `next` (None:
    /// the chain is spent until the person is back).
    Raise { next: Option<i64> },
}

pub fn step(plan: &Plan, zone: &Zone, now: i64) -> Step {
    let oneoff_due = plan.oneoff_at.is_some_and(|t| t <= now);
    let chain_due = plan.enabled && plan.next_due_at.is_some_and(|t| t <= now);
    let chain_next = || (plan.unanswered + 1 < MAX_UNANSWERED).then(|| next_after(plan, zone, now));
    if oneoff_due {
        // Asked for at this time: raised even inside quiet hours. A chain
        // check-in due at the same moment is covered by this one.
        let next = if chain_due {
            chain_next()
        } else {
            plan.next_due_at
        };
        return Step::Raise { next };
    }
    if !chain_due {
        return Step::Wait;
    }
    if let Some((_, until)) = plan.quiet {
        if in_quiet(plan.quiet, time::minute_of_day(zone, now)) {
            return Step::Defer {
                until: quiet_end_after(zone, until, now),
            };
        }
    }
    Step::Raise { next: chain_next() }
}

/// Apply a step to the plan (the check-in row itself is the store's).
pub fn apply(plan: &mut Plan, step: &Step, now: i64) {
    match step {
        Step::Wait => {}
        Step::Defer { until } => plan.next_due_at = Some(*until),
        Step::Raise { next } => {
            plan.unanswered += 1;
            plan.next_due_at = *next;
            if plan.oneoff_at.is_some_and(|t| t <= now) {
                plan.oneoff_at = None;
            }
        }
    }
}

/// A check-in's notice: title and body, from templates. No model is asked,
/// and moods are never part of it.
pub fn notice_text(task: Option<(&str, i64)>, loose_end: Option<&str>) -> (String, String) {
    let mut body = match task {
        Some((title, minutes)) => format!("Still on {title}? {} in.", time::span(minutes)),
        None => "What are you working on?".to_string(),
    };
    if let Some(note) = loose_end {
        let short: String = note.chars().take(120).collect();
        let more = if note.chars().count() > 120 {
            "…"
        } else {
            ""
        };
        body.push_str(&format!("\nLoose end: {short}{more}"));
    }
    ("Check-in".to_string(), body)
}

/// The notice subject: a newer check-in replaces the older unread one, and
/// answering retires it.
pub fn notice_subject(user_id: &str) -> String {
    let subject = format!("body-double/{user_id}/checkin");
    subject.chars().take(200).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use greenhouse_kernel::clock;

    fn at(y: i64, m: u32, d: u32, h: i64, min: i64) -> i64 {
        clock::days_from_civil(y, m, d) * 86_400 + h * 3600 + min * 60
    }

    fn plan() -> Plan {
        Plan::new("owner", &PlanDefaults::default(), "UTC")
    }

    #[test]
    fn quiet_windows_may_wrap_midnight() {
        let night = Some((22 * 60, 8 * 60));
        assert!(in_quiet(night, 23 * 60));
        assert!(in_quiet(night, 0));
        assert!(in_quiet(night, 7 * 60 + 59));
        assert!(!in_quiet(night, 8 * 60));
        assert!(!in_quiet(night, 21 * 60 + 59));
        let lunch = Some((12 * 60, 13 * 60));
        assert!(in_quiet(lunch, 12 * 60 + 30));
        assert!(!in_quiet(lunch, 13 * 60));
        assert!(!in_quiet(None, 0));
        assert!(!in_quiet(Some((600, 600)), 600));
    }

    #[test]
    fn activity_schedules_the_next_check_in_outside_quiet_hours() {
        let utc = Zone::utc();
        let mut p = plan();
        p.unanswered = 3;
        after_activity(&mut p, &utc, at(2026, 10, 15, 14, 0));
        assert_eq!(p.unanswered, 0);
        assert_eq!(p.next_due_at, Some(at(2026, 10, 15, 14, 45)));

        // 21:30 + 45 min lands at 22:15, inside quiet hours: 08:00 tomorrow.
        after_activity(&mut p, &utc, at(2026, 10, 15, 21, 30));
        assert_eq!(p.next_due_at, Some(at(2026, 10, 16, 8, 0)));

        // Off: nothing is scheduled.
        p.enabled = false;
        after_activity(&mut p, &utc, at(2026, 10, 15, 14, 0));
        assert_eq!(p.next_due_at, None);
    }

    #[test]
    fn a_due_check_in_is_raised_and_the_next_one_follows() {
        let utc = Zone::utc();
        let mut p = plan();
        let now = at(2026, 10, 15, 15, 0);
        assert_eq!(step(&p, &utc, now), Step::Wait, "nothing scheduled yet");
        p.next_due_at = Some(now);
        assert_eq!(
            step(&p, &utc, now),
            Step::Raise {
                next: Some(at(2026, 10, 15, 15, 45))
            }
        );
    }

    #[test]
    fn quiet_hours_defer_without_raising() {
        let utc = Zone::utc();
        let mut p = plan();
        let now = at(2026, 10, 15, 23, 10);
        p.next_due_at = Some(now);
        let s = step(&p, &utc, now);
        assert_eq!(
            s,
            Step::Defer {
                until: at(2026, 10, 16, 8, 0)
            }
        );
        apply(&mut p, &s, now);
        assert_eq!(p.unanswered, 0, "a deferral is not a check-in");
        assert_eq!(p.next_due_at, Some(at(2026, 10, 16, 8, 0)));
    }

    #[test]
    fn a_check_in_asked_for_comes_even_in_quiet_hours_and_off() {
        let utc = Zone::utc();
        let mut p = plan();
        p.enabled = false;
        let now = at(2026, 10, 15, 23, 25);
        p.oneoff_at = Some(now);
        assert_eq!(p.next(), Some(now));
        let s = step(&p, &utc, now);
        assert_eq!(s, Step::Raise { next: None });
        apply(&mut p, &s, now);
        assert_eq!(p.oneoff_at, None);
        assert_eq!(p.unanswered, 1);
        assert_eq!(step(&p, &utc, now + 60), Step::Wait);
    }

    #[test]
    fn activity_does_not_move_a_check_in_asked_for() {
        let utc = Zone::utc();
        let mut p = plan();
        p.oneoff_at = Some(at(2026, 10, 15, 14, 25));
        after_activity(&mut p, &utc, at(2026, 10, 15, 14, 10));
        assert_eq!(p.oneoff_at, Some(at(2026, 10, 15, 14, 25)));
        assert_eq!(p.next(), Some(at(2026, 10, 15, 14, 25)));
    }

    #[test]
    fn eight_unanswered_in_a_row_end_the_chain() {
        let utc = Zone::utc();
        let mut p = plan();
        p.quiet = None;
        let mut now = at(2026, 10, 15, 9, 0);
        p.next_due_at = Some(now);
        for n in 1..=MAX_UNANSWERED {
            let s = step(&p, &utc, now);
            apply(&mut p, &s, now);
            assert_eq!(p.unanswered, n);
            if n < MAX_UNANSWERED {
                now = p.next_due_at.expect("the chain continues");
            }
        }
        assert_eq!(p.next_due_at, None, "quiet until the person is back");
        after_activity(&mut p, &utc, now + 600);
        assert!(p.next_due_at.is_some());
    }

    #[test]
    fn quiet_hours_end_in_the_persons_zone_across_daylight_saving() {
        let Ok(ny) = Zone::load("America/New_York") else {
            return; // no zone database on this host
        };
        // 2026-11-01 is the end of daylight saving in New York. 23:30 EDT on
        // Oct 31 is 03:30 UTC Nov 1; quiet ends at 08:00 EST = 13:00 UTC.
        let now = at(2026, 11, 1, 3, 30);
        assert_eq!(quiet_end_after(&ny, 8 * 60, now), at(2026, 11, 1, 13, 0));
    }

    #[test]
    fn the_notice_is_a_template_without_moods() {
        let (title, body) = notice_text(Some(("WGU assignment", 130)), None);
        assert_eq!(title, "Check-in");
        assert_eq!(body, "Still on WGU assignment? 2h 10m in.");
        let (_, body) = notice_text(None, Some("email Dana re: schema review"));
        assert_eq!(
            body,
            "What are you working on?\nLoose end: email Dana re: schema review"
        );
        assert_eq!(notice_subject("owner"), "body-double/owner/checkin");
    }
}
