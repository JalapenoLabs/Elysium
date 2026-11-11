// Copyright © 2026 Jalapeno Labs

//! When each Project Manager's next turn is due: the debounce over its waiting wakes.
//!
//! A meeting lands many segments on a project at once, and a person tidying a timeline moves
//! records one after another, so a turn waits for its project's wakes to go quiet
//! ([`QUIET_WINDOW`]) and takes them all together. A steady trickle never goes quiet, so a turn
//! is due at the latest [`MAX_WAIT`] after the oldest wake arrived. A person's or Elysia's
//! message is due at once: someone is waiting for the answer. An action item a person assigns
//! waits like a record, since a person assigns a selection, or several items one after another,
//! and one turn plans them together.
//!
//! A project with a turn running is not due: what arrives meanwhile waits for the turn after.
//! A project a usage limit holds is due when the hold lifts. Everything here is a pure function
//! of what waits and when, tested on its own; the dispatcher only reads the rows and acts.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, TimeDelta, Utc};
use uuid::Uuid;

use crate::models::project_manager::{WaitingWake, WakeKind};

/// How long a project's wakes must be quiet before its turn starts. Segmenting a meeting or a
/// conversation lands its records in one transaction; thirty seconds also gathers a person's
/// run of corrections, as Elysia's intake gathers a burst of changes.
pub const QUIET_WINDOW: TimeDelta = TimeDelta::seconds(30);

/// The longest a waiting wake holds its turn back, however busy its project stays.
pub const MAX_WAIT: TimeDelta = TimeDelta::minutes(2);

/// What is due now, and when to look again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The projects whose turn may start now, the longest waiting first.
    pub due: Vec<Uuid>,
    /// When the next project not due now becomes due; `None` when nothing waits for time.
    pub next_due_at: Option<DateTime<Utc>>,
}

/// Plans from `waiting`, every waiting wake oldest first, given the projects with a turn
/// `running` and those a usage limit `held` until a time, at `now`.
pub fn plan(
    waiting: &[WaitingWake],
    running: &HashSet<Uuid>,
    held: &HashMap<Uuid, DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Plan {
    // One pass gathers, per project in the order its oldest wake arrived, what its due time
    // depends on.
    let mut order: Vec<Uuid> = Vec::new();
    let mut projects: HashMap<Uuid, Waiting> = HashMap::new();
    for wake in waiting {
        if running.contains(&wake.project_id) {
            continue;
        }
        let project = projects.entry(wake.project_id).or_insert_with(|| {
            order.push(wake.project_id);
            Waiting {
                oldest: wake.created_at,
                newest: wake.created_at,
                first_message: None,
            }
        });
        project.oldest = project.oldest.min(wake.created_at);
        project.newest = project.newest.max(wake.created_at);
        if wake.kind == WakeKind::Message {
            let first = project.first_message.get_or_insert(wake.created_at);
            *first = (*first).min(wake.created_at);
        }
    }

    let mut due = Vec::new();
    let mut next_due_at: Option<DateTime<Utc>> = None;
    for project_id in order {
        let project = &projects[&project_id];
        let quiet = (project.newest + QUIET_WINDOW).min(project.oldest + MAX_WAIT);
        let mut at = project
            .first_message
            .map_or(quiet, |message| message.min(quiet));
        if let Some(until) = held.get(&project_id) {
            at = at.max(*until);
        }
        if at <= now {
            due.push(project_id);
        } else {
            next_due_at = Some(next_due_at.map_or(at, |next| next.min(at)));
        }
    }
    Plan { due, next_due_at }
}

/// What one project's waiting wakes say about when its turn is due.
struct Waiting {
    oldest: DateTime<Utc>,
    newest: DateTime<Utc>,
    /// When the first waiting message arrived, which makes the turn due at once.
    first_message: Option<DateTime<Utc>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wake(project_id: Uuid, kind: WakeKind, created_at: DateTime<Utc>) -> WaitingWake {
        WaitingWake {
            project_id,
            kind,
            created_at,
        }
    }

    fn nothing_running() -> HashSet<Uuid> {
        HashSet::new()
    }

    fn nothing_held() -> HashMap<Uuid, DateTime<Utc>> {
        HashMap::new()
    }

    #[test]
    fn a_burst_of_segments_waits_until_it_goes_quiet_and_is_one_turn() {
        let project = Uuid::now_v7();
        let start = Utc::now();
        let burst: Vec<WaitingWake> = (0..10)
            .map(|second| {
                wake(
                    project,
                    WakeKind::Record,
                    start + TimeDelta::seconds(second),
                )
            })
            .collect();

        let quiet_from = start + TimeDelta::seconds(9) + QUIET_WINDOW;
        let early = plan(
            &burst,
            &nothing_running(),
            &nothing_held(),
            quiet_from - TimeDelta::seconds(1),
        );
        assert!(early.due.is_empty(), "still arriving");
        assert_eq!(early.next_due_at, Some(quiet_from));

        let quiet = plan(&burst, &nothing_running(), &nothing_held(), quiet_from);
        assert_eq!(quiet.due, vec![project], "one turn for the whole burst");
        assert_eq!(quiet.next_due_at, None);
    }

    #[test]
    fn a_steady_trickle_is_due_at_the_longest_wait() {
        let project = Uuid::now_v7();
        let start = Utc::now();
        // One every twenty seconds never goes quiet for thirty.
        let trickle: Vec<WaitingWake> = (0..12)
            .map(|step| {
                wake(
                    project,
                    WakeKind::Record,
                    start + TimeDelta::seconds(step * 20),
                )
            })
            .collect();

        let planned = plan(
            &trickle,
            &nothing_running(),
            &nothing_held(),
            start + MAX_WAIT,
        );
        assert_eq!(planned.due, vec![project]);
    }

    #[test]
    fn a_message_is_due_at_once_and_takes_the_waiting_records_with_it() {
        let project = Uuid::now_v7();
        let now = Utc::now();
        let waiting = [
            wake(project, WakeKind::Record, now - TimeDelta::seconds(5)),
            wake(project, WakeKind::Message, now),
        ];
        let planned = plan(&waiting, &nothing_running(), &nothing_held(), now);
        assert_eq!(planned.due, vec![project]);
    }

    #[test]
    fn items_assigned_one_after_another_wait_to_be_planned_in_one_turn() {
        let project = Uuid::now_v7();
        let start = Utc::now();
        let assigned = [
            wake(project, WakeKind::Assignment, start),
            wake(
                project,
                WakeKind::Assignment,
                start + TimeDelta::seconds(10),
            ),
        ];
        let quiet_from = start + TimeDelta::seconds(10) + QUIET_WINDOW;

        let early = plan(&assigned, &nothing_running(), &nothing_held(), start);
        assert!(early.due.is_empty(), "an assignment is not due at once");
        assert_eq!(early.next_due_at, Some(quiet_from));
        let quiet = plan(&assigned, &nothing_running(), &nothing_held(), quiet_from);
        assert_eq!(quiet.due, vec![project]);
    }

    #[test]
    fn a_running_turn_holds_its_projects_wakes_for_the_next_one() {
        let busy = Uuid::now_v7();
        let free = Uuid::now_v7();
        let now = Utc::now();
        let waiting = [
            wake(busy, WakeKind::Message, now - TimeDelta::minutes(5)),
            wake(free, WakeKind::Message, now),
        ];
        let running = HashSet::from([busy]);
        let planned = plan(&waiting, &running, &nothing_held(), now);
        assert_eq!(planned.due, vec![free]);
        assert_eq!(
            planned.next_due_at, None,
            "the running turn's end looks again"
        );
    }

    #[test]
    fn a_usage_limit_holds_a_project_until_it_lifts() {
        let project = Uuid::now_v7();
        let now = Utc::now();
        let lifts = now + TimeDelta::hours(2);
        let waiting = [wake(project, WakeKind::Message, now)];
        let held = HashMap::from([(project, lifts)]);

        let planned = plan(&waiting, &nothing_running(), &held, now);
        assert!(planned.due.is_empty());
        assert_eq!(planned.next_due_at, Some(lifts));
        let later = plan(&waiting, &nothing_running(), &held, lifts);
        assert_eq!(later.due, vec![project]);
    }

    #[test]
    fn the_longest_waiting_project_goes_first() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let now = Utc::now();
        let waiting = [
            wake(second, WakeKind::Batch, now - TimeDelta::minutes(4)),
            wake(first, WakeKind::Record, now - TimeDelta::minutes(3)),
        ];
        let planned = plan(&waiting, &nothing_running(), &nothing_held(), now);
        assert_eq!(planned.due, vec![second, first]);
    }
}
