// Copyright © 2026 Jalapeno Labs

//! What a Project Manager is doing, at a glance, derived on every read from its turns and wakes.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// What a project's page shows of its Project Manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// A turn is running.
    Running,
    /// Something waits for it, and a usage limit holds it until a time.
    Held,
    /// Its last turn failed.
    Errored,
    /// Something waits for it, and its next turn starts once it goes quiet.
    Waiting,
    /// Nothing waits for it.
    Idle,
}

/// What [`derive`] reads.
#[derive(Debug, Clone, Copy)]
pub struct StatusInputs {
    pub is_running: bool,
    /// How many wakes wait for its next turn.
    pub waiting: i64,
    pub usage_paused_until: Option<DateTime<Utc>>,
    pub last_turn_failed: bool,
    pub now: DateTime<Utc>,
}

/// Its status: running first, then a hold on work that waits, then a failure, then waiting work.
pub fn derive(inputs: &StatusInputs) -> Status {
    let is_held = inputs
        .usage_paused_until
        .is_some_and(|until| until > inputs.now);
    if inputs.is_running {
        return Status::Running;
    }
    if inputs.waiting > 0 && is_held {
        return Status::Held;
    }
    if inputs.last_turn_failed {
        return Status::Errored;
    }
    if inputs.waiting > 0 {
        return Status::Waiting;
    }
    Status::Idle
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;

    fn inputs() -> StatusInputs {
        StatusInputs {
            is_running: false,
            waiting: 0,
            usage_paused_until: None,
            last_turn_failed: false,
            now: Utc::now(),
        }
    }

    #[test]
    fn running_wins_then_a_hold_then_a_failure_then_waiting_work() {
        let now = Utc::now();
        let everything = StatusInputs {
            is_running: true,
            waiting: 3,
            usage_paused_until: Some(now + TimeDelta::hours(1)),
            last_turn_failed: true,
            now,
        };
        assert_eq!(derive(&everything), Status::Running);
        let held = StatusInputs {
            is_running: false,
            ..everything
        };
        assert_eq!(derive(&held), Status::Held);
        let failed = StatusInputs {
            usage_paused_until: None,
            ..held
        };
        assert_eq!(derive(&failed), Status::Errored);
        let waiting = StatusInputs {
            last_turn_failed: false,
            ..failed
        };
        assert_eq!(derive(&waiting), Status::Waiting);
        assert_eq!(derive(&inputs()), Status::Idle);
    }

    #[test]
    fn a_hold_with_nothing_waiting_or_one_that_lifted_holds_nothing() {
        let now = Utc::now();
        let nothing_waits = StatusInputs {
            usage_paused_until: Some(now + TimeDelta::hours(1)),
            ..inputs()
        };
        assert_eq!(derive(&nothing_waits), Status::Idle);
        let lifted = StatusInputs {
            waiting: 2,
            usage_paused_until: Some(now - TimeDelta::minutes(1)),
            now,
            ..inputs()
        };
        assert_eq!(derive(&lifted), Status::Waiting);
    }
}
