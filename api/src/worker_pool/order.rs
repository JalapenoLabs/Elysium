// Copyright © 2026 Jalapeno Labs

//! The order the pool hands out slots in: a pure function of what waits, how many slots each
//! project holds, and the time.
//!
//! 1. **Run next.** A batch someone asked to run next goes before everything else, the earliest
//!    asked first.
//! 2. **Tiers.** Then due scheduled batches, then ASAP, then normal. A scheduled batch is not
//!    handed a slot before its time.
//! 3. **Round robin.** Within a tier, the next slot goes to the project holding the fewest slots,
//!    counting those it holds already and those it is handed ahead in this order, so a project
//!    that submits twenty batches never starves one that submits one. Each project's own batches
//!    go oldest first (a scheduled one by when it was due), and a tie between projects goes to the
//!    one whose next batch has waited longest.
//!
//! The order is derived again on every look, from rows alone, so an engine that restarts hands
//! out slots exactly as its predecessor would have.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, VecDeque};

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::models::work_batch::{Priority, WorkBatch};

/// What the order reads of a waiting batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waiting {
    pub id: Uuid,
    pub project_id: Uuid,
    pub priority: Priority,
    pub scheduled_for: Option<DateTime<Utc>>,
    pub pinned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<&WorkBatch> for Waiting {
    fn from(batch: &WorkBatch) -> Self {
        Self {
            id: batch.id,
            project_id: batch.project_id,
            priority: batch.priority,
            scheduled_for: batch.scheduled_for,
            pinned_at: batch.pinned_at,
            created_at: batch.created_at,
        }
    }
}

/// The tiers after the batches asked to run next, in the order they are served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Tier {
    DueScheduled,
    Asap,
    Normal,
}

/// How long a batch has waited within its tier, older first: a scheduled batch by when it was
/// due, every other by when it was submitted, then by id so the order is total.
type Age = (DateTime<Utc>, Uuid);

/// The order the pool hands slots to `waiting` at `now`, given how many slots each project holds
/// already (`in_slots`, a project absent holding none). Answers the ids of every batch that may
/// be handed a slot, first first; a scheduled batch not yet due is left out, as is anything not
/// queued. A caller with `n` free slots hands them to the first `n`.
pub fn order(
    waiting: &[Waiting],
    in_slots: &HashMap<Uuid, usize>,
    now: DateTime<Utc>,
) -> Vec<Uuid> {
    let mut held = in_slots.clone();
    let mut ordered = Vec::with_capacity(waiting.len());

    let mut run_next: Vec<(DateTime<Utc>, Uuid, Uuid)> = Vec::new();
    let mut tiers: HashMap<Tier, HashMap<Uuid, Vec<(Age, Uuid)>>> = HashMap::new();
    for batch in waiting {
        if let Some(pinned_at) = batch.pinned_at {
            run_next.push((pinned_at, batch.id, batch.project_id));
            continue;
        }
        let Some((tier, age)) = tier_of(batch, now) else {
            continue;
        };
        tiers
            .entry(tier)
            .or_default()
            .entry(batch.project_id)
            .or_default()
            .push((age, batch.id));
    }

    run_next.sort_unstable();
    for (_pinned_at, id, project_id) in run_next {
        ordered.push(id);
        *held.entry(project_id).or_default() += 1;
    }

    for tier in [Tier::DueScheduled, Tier::Asap, Tier::Normal] {
        let Some(lanes) = tiers.remove(&tier) else {
            continue;
        };
        round_robin(lanes, &mut held, &mut ordered);
    }
    ordered
}

/// The tier `batch` waits in at `now` and how long it has waited there; `None` for a scheduled
/// batch not yet due.
fn tier_of(batch: &Waiting, now: DateTime<Utc>) -> Option<(Tier, Age)> {
    match (batch.priority, batch.scheduled_for) {
        (Priority::Scheduled, Some(due)) if due <= now => {
            Some((Tier::DueScheduled, (due, batch.id)))
        }
        (Priority::Scheduled, _) => None,
        (Priority::Asap, _) => Some((Tier::Asap, (batch.created_at, batch.id))),
        (Priority::Normal, _) => Some((Tier::Normal, (batch.created_at, batch.id))),
    }
}

/// Serves one tier's lanes, a project's batches each, into `ordered`: always the project holding
/// the fewest slots next, a tie going to the lane whose next batch has waited longest. Every
/// batch served counts toward its project's slots for the tiers after.
fn round_robin(
    lanes: HashMap<Uuid, Vec<(Age, Uuid)>>,
    held: &mut HashMap<Uuid, usize>,
    ordered: &mut Vec<Uuid>,
) {
    let mut queues: HashMap<Uuid, VecDeque<(Age, Uuid)>> = HashMap::with_capacity(lanes.len());
    // Lowest first: the fewest slots held, then the longest waiting.
    let mut next: BinaryHeap<Reverse<(usize, Age, Uuid)>> = BinaryHeap::new();
    for (project_id, mut lane) in lanes {
        lane.sort_unstable();
        let lane = VecDeque::from(lane);
        if let Some((age, _id)) = lane.front() {
            let slots = held.get(&project_id).copied().unwrap_or_default();
            next.push(Reverse((slots, *age, project_id)));
        }
        queues.insert(project_id, lane);
    }
    while let Some(Reverse((_slots, _age, project_id))) = next.pop() {
        let Some(lane) = queues.get_mut(&project_id) else {
            continue;
        };
        let Some((_age, id)) = lane.pop_front() else {
            continue;
        };
        ordered.push(id);
        let slots = held.entry(project_id).or_default();
        *slots += 1;
        if let Some((age, _id)) = lane.front() {
            next.push(Reverse((*slots, *age, project_id)));
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    /// Builds batches whose ids and times follow the order they are made in.
    struct Batches {
        start: DateTime<Utc>,
        made: i64,
    }

    impl Batches {
        fn new() -> Self {
            Self {
                start: Utc::now() - TimeDelta::days(1),
                made: 0,
            }
        }

        /// A batch of `project_id` with `priority`, submitted a minute after the one before.
        fn batch(&mut self, project_id: Uuid, priority: Priority) -> Waiting {
            self.made += 1;
            Waiting {
                id: Uuid::now_v7(),
                project_id,
                priority,
                scheduled_for: None,
                pinned_at: None,
                created_at: self.start + TimeDelta::minutes(self.made),
            }
        }

        fn scheduled(&mut self, project_id: Uuid, due: DateTime<Utc>) -> Waiting {
            Waiting {
                scheduled_for: Some(due),
                ..self.batch(project_id, Priority::Scheduled)
            }
        }
    }

    fn ids(batches: &[&Waiting]) -> Vec<Uuid> {
        batches.iter().map(|batch| batch.id).collect()
    }

    #[test]
    fn nothing_waiting_orders_nothing() {
        assert!(order(&[], &HashMap::new(), Utc::now()).is_empty());
    }

    #[test]
    fn one_projects_batches_go_oldest_first() {
        let mut made = Batches::new();
        let project = Uuid::now_v7();
        let first = made.batch(project, Priority::Normal);
        let second = made.batch(project, Priority::Normal);
        let third = made.batch(project, Priority::Normal);
        let waiting = [third, first, second];

        assert_eq!(
            order(&waiting, &HashMap::new(), Utc::now()),
            ids(&[&first, &second, &third])
        );
    }

    #[test]
    fn asap_goes_before_normal_however_old_the_normal_batch() {
        let mut made = Batches::new();
        let project = Uuid::now_v7();
        let old_normal = made.batch(project, Priority::Normal);
        let new_asap = made.batch(project, Priority::Asap);

        assert_eq!(
            order(&[old_normal, new_asap], &HashMap::new(), Utc::now()),
            ids(&[&new_asap, &old_normal])
        );
    }

    #[test]
    fn due_scheduled_work_jumps_ahead_of_asap_and_waits_for_its_time() {
        let now = Utc::now();
        let mut made = Batches::new();
        let project = Uuid::now_v7();
        let asap = made.batch(project, Priority::Asap);
        let due = made.scheduled(project, now - TimeDelta::minutes(5));
        let later = made.scheduled(project, now + TimeDelta::hours(1));
        let normal = made.batch(project, Priority::Normal);
        let waiting = [asap, due, later, normal];

        assert_eq!(
            order(&waiting, &HashMap::new(), now),
            ids(&[&due, &asap, &normal]),
            "the one due goes first; the one not yet due is left out"
        );
        assert_eq!(
            order(&waiting, &HashMap::new(), now + TimeDelta::hours(2)),
            ids(&[&due, &later, &asap, &normal]),
            "once its time comes it jumps ahead too"
        );
    }

    #[test]
    fn due_scheduled_batches_go_in_the_order_they_came_due() {
        let now = Utc::now();
        let mut made = Batches::new();
        let project = Uuid::now_v7();
        // Submitted first, but due later.
        let due_recently = made.scheduled(project, now - TimeDelta::minutes(1));
        let due_long_ago = made.scheduled(project, now - TimeDelta::hours(3));

        assert_eq!(
            order(&[due_recently, due_long_ago], &HashMap::new(), now),
            ids(&[&due_long_ago, &due_recently])
        );
    }

    #[test]
    fn projects_take_turns_within_a_tier() {
        let mut made = Batches::new();
        let busy = Uuid::now_v7();
        let quiet = Uuid::now_v7();
        let third = Uuid::now_v7();
        // `busy` submitted a burst before the others submitted anything.
        let busy_1 = made.batch(busy, Priority::Normal);
        let busy_2 = made.batch(busy, Priority::Normal);
        let busy_3 = made.batch(busy, Priority::Normal);
        let quiet_1 = made.batch(quiet, Priority::Normal);
        let third_1 = made.batch(third, Priority::Normal);
        let quiet_2 = made.batch(quiet, Priority::Normal);
        let waiting = [busy_1, busy_2, busy_3, quiet_1, third_1, quiet_2];

        assert_eq!(
            order(&waiting, &HashMap::new(), Utc::now()),
            ids(&[&busy_1, &quiet_1, &third_1, &busy_2, &quiet_2, &busy_3]),
            "one each in turn, the longest waiting first in each round"
        );
    }

    #[test]
    fn a_project_holding_slots_yields_to_one_holding_none() {
        let mut made = Batches::new();
        let hogging = Uuid::now_v7();
        let waiting_project = Uuid::now_v7();
        let hogging_next = made.batch(hogging, Priority::Asap);
        let other_1 = made.batch(waiting_project, Priority::Asap);
        let other_2 = made.batch(waiting_project, Priority::Asap);
        let in_slots = HashMap::from([(hogging, 2)]);

        assert_eq!(
            order(&[hogging_next, other_1, other_2], &in_slots, Utc::now()),
            ids(&[&other_1, &other_2, &hogging_next]),
            "the project already running two waits until the other has caught up"
        );
    }

    #[test]
    fn slots_handed_out_in_an_earlier_tier_count_in_the_next() {
        let mut made = Batches::new();
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let first_asap = made.batch(first, Priority::Asap);
        let first_normal = made.batch(first, Priority::Normal);
        let second_normal = made.batch(second, Priority::Normal);

        assert_eq!(
            order(
                &[first_asap, first_normal, second_normal],
                &HashMap::new(),
                Utc::now()
            ),
            ids(&[&first_asap, &second_normal, &first_normal]),
            "having taken a slot for its ASAP batch, the first project's normal one waits its turn"
        );
    }

    #[test]
    fn batches_asked_to_run_next_go_first_in_the_order_asked() {
        let now = Utc::now();
        let mut made = Batches::new();
        let project = Uuid::now_v7();
        let due = made.scheduled(project, now - TimeDelta::minutes(5));
        let asap = made.batch(project, Priority::Asap);
        let mut asked_second = made.batch(project, Priority::Normal);
        asked_second.pinned_at = Some(now - TimeDelta::seconds(10));
        let mut asked_first = made.batch(project, Priority::Normal);
        asked_first.pinned_at = Some(now - TimeDelta::seconds(30));
        let waiting = [due, asap, asked_second, asked_first];

        assert_eq!(
            order(&waiting, &HashMap::new(), now),
            ids(&[&asked_first, &asked_second, &due, &asap])
        );
    }

    #[test]
    fn batches_run_next_count_toward_their_projects_turn() {
        let now = Utc::now();
        let mut made = Batches::new();
        let pinned_project = Uuid::now_v7();
        let other = Uuid::now_v7();
        let mut pinned = made.batch(pinned_project, Priority::Normal);
        pinned.pinned_at = Some(now);
        let pinned_project_next = made.batch(pinned_project, Priority::Normal);
        let other_next = made.batch(other, Priority::Normal);

        assert_eq!(
            order(
                &[pinned, pinned_project_next, other_next],
                &HashMap::new(),
                now
            ),
            ids(&[&pinned, &other_next, &pinned_project_next])
        );
    }

    #[test]
    fn the_order_is_the_same_however_the_batches_arrive() {
        let now = Utc::now();
        let mut made = Batches::new();
        let projects = [Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7()];
        let priorities = [
            Priority::Normal,
            Priority::Asap,
            Priority::Normal,
            Priority::Asap,
        ];
        let mut waiting = Vec::new();
        for index in 0..24 {
            let project = projects[index % projects.len()];
            waiting.push(made.batch(project, priorities[index % priorities.len()]));
        }
        waiting.push(made.scheduled(projects[0], now - TimeDelta::minutes(1)));
        let in_slots = HashMap::from([(projects[1], 1)]);

        let expected = order(&waiting, &in_slots, now);
        waiting.reverse();
        assert_eq!(order(&waiting, &in_slots, now), expected);
        waiting.rotate_left(7);
        assert_eq!(order(&waiting, &in_slots, now), expected);
        assert_eq!(expected.len(), waiting.len(), "every batch is ordered once");
    }

    #[test]
    fn every_project_gets_a_fair_share_of_the_first_slots() {
        let mut made = Batches::new();
        let projects: Vec<Uuid> = (0..4).map(|_index| Uuid::now_v7()).collect();
        let mut waiting = Vec::new();
        // The first project floods the queue before anyone else submits.
        for _index in 0..20 {
            waiting.push(made.batch(projects[0], Priority::Normal));
        }
        for project in &projects[1..] {
            for _index in 0..3 {
                waiting.push(made.batch(*project, Priority::Normal));
            }
        }
        let by_id: HashMap<Uuid, Uuid> = waiting
            .iter()
            .map(|batch| (batch.id, batch.project_id))
            .collect();

        let first_eight: Vec<Uuid> = order(&waiting, &HashMap::new(), Utc::now())
            .into_iter()
            .take(8)
            .map(|id| by_id[&id])
            .collect();
        for project in &projects {
            assert_eq!(
                first_eight.iter().filter(|owner| *owner == project).count(),
                2,
                "eight slots across four projects is two each"
            );
        }
    }
}
