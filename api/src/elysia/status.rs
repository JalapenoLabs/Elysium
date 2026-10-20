// Copyright © 2026 Jalapeno Labs

//! Elysia's status at a glance: the one color the interface shows for her, and what, if
//! anything, is holding her back.
//!
//! The color follows the vision (`docs/vision.md`): orange while she runs, red when her last
//! turn failed, pink when she needs a person, green when she is done. When more than one
//! applies, needing a person wins, because it is the one only a person can clear; then
//! running, because what she does now supersedes how her last turn ended.

use chrono::{DateTime, Utc};
use serde::Serialize;

/// The color Elysia shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Light {
    /// A turn is running. Orange.
    Running,
    /// Her last turn failed and none has succeeded since. Red.
    Errored,
    /// She asked a question nobody has answered. Pink.
    NeedsYou,
    /// Nothing is running, failing, or waiting on a person. Green.
    Done,
}

/// Why queued work is not starting. Only one is reported: the first that a person would have
/// to clear.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Hold {
    /// A person paused her.
    Paused,
    /// Her subscriptions reported a usage limit; work resumes at `until`.
    UsageLimit { until: DateTime<Utc> },
    /// Nothing can run her turns on this deployment yet.
    NoRunner,
    /// Outside the hours she may start work on her own. A person's message still starts a turn.
    Unavailable,
}

/// What the status is derived from, read from the database and the runner.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is an independent fact read from a different place"
)]
#[derive(Debug, Clone, Copy)]
pub struct StatusInputs {
    pub is_running: bool,
    pub last_turn_failed: bool,
    pub open_questions: usize,
    pub paused: bool,
    pub usage_paused_until: Option<DateTime<Utc>>,
    pub runner_available: bool,
    pub available: bool,
    pub now: DateTime<Utc>,
}

/// Elysia's color and hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub light: Light,
    pub hold: Option<Hold>,
}

/// Derives the status from `inputs`.
pub fn derive(inputs: &StatusInputs) -> Status {
    let light = if inputs.open_questions > 0 {
        Light::NeedsYou
    } else if inputs.is_running {
        Light::Running
    } else if inputs.last_turn_failed {
        Light::Errored
    } else {
        Light::Done
    };

    let usage_limit = inputs
        .usage_paused_until
        .filter(|until| *until > inputs.now)
        .map(|until| Hold::UsageLimit { until });
    let hold = if inputs.paused {
        Some(Hold::Paused)
    } else if usage_limit.is_some() {
        usage_limit
    } else if !inputs.runner_available {
        Some(Hold::NoRunner)
    } else if !inputs.available {
        Some(Hold::Unavailable)
    } else {
        None
    };

    Status { light, hold }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn idle() -> StatusInputs {
        StatusInputs {
            is_running: false,
            last_turn_failed: false,
            open_questions: 0,
            paused: false,
            usage_paused_until: None,
            runner_available: true,
            available: true,
            now: Utc::now(),
        }
    }

    #[test]
    fn nothing_to_do_is_green_and_unheld() {
        assert_eq!(
            derive(&idle()),
            Status {
                light: Light::Done,
                hold: None
            }
        );
    }

    #[test]
    fn a_question_outranks_running_and_running_outranks_a_failure() {
        let failed = StatusInputs {
            last_turn_failed: true,
            ..idle()
        };
        assert_eq!(derive(&failed).light, Light::Errored);

        let running = StatusInputs {
            is_running: true,
            ..failed
        };
        assert_eq!(derive(&running).light, Light::Running);

        let asking = StatusInputs {
            open_questions: 1,
            ..running
        };
        assert_eq!(derive(&asking).light, Light::NeedsYou);
    }

    #[test]
    fn a_pause_outranks_every_other_hold() {
        let inputs = StatusInputs {
            paused: true,
            usage_paused_until: Some(Utc::now() + Duration::hours(1)),
            runner_available: false,
            available: false,
            ..idle()
        };
        assert_eq!(derive(&inputs).hold, Some(Hold::Paused));
    }

    #[test]
    fn a_usage_limit_holds_only_until_it_resets() {
        let now = Utc::now();
        let until = now + Duration::minutes(30);
        let limited = StatusInputs {
            usage_paused_until: Some(until),
            now,
            ..idle()
        };
        assert_eq!(derive(&limited).hold, Some(Hold::UsageLimit { until }));

        let reset = StatusInputs {
            now: until,
            ..limited
        };
        assert_eq!(derive(&reset).hold, None);
    }

    #[test]
    fn a_missing_runner_is_named_before_her_hours() {
        let inputs = StatusInputs {
            runner_available: false,
            available: false,
            ..idle()
        };
        assert_eq!(derive(&inputs).hold, Some(Hold::NoRunner));

        let off_hours = StatusInputs {
            available: false,
            ..idle()
        };
        assert_eq!(derive(&off_hours).hold, Some(Hold::Unavailable));
    }
}
