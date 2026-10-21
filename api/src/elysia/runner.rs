// Copyright © 2026 Jalapeno Labs

//! The seam between Elysia's dispatcher and what runs her turns.
//!
//! Her turns run on the host runner ([`crate::runner`]), which executes the official Claude
//! Code CLI in Elysium's workspace container. This module is the one place that knows it: the
//! dispatcher hands over an [`ElysiaTurn`] and gets back an [`ElysiaOutcome`] in her own terms.
//!
//! All her turns share one runner conversation, so the runner never starts two at once, and
//! one working directory in the workspace, `elysia/`, where her files persist between turns.
//!
//! [`TurnRunner`] is an enum rather than a trait object, following the repository's Rust
//! guidelines: the host runner, plus a scripted one for tests.

use chrono::{DateTime, TimeDelta, Utc};
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use crate::models::user::ELYSIA_USER_ID;
use crate::runner::{
    self, ConversationId, HarnessSessionId, RunnerEvent, TurnMode, TurnOutcome, TurnRequest,
    WorkspacePath,
};

/// Where her turns run, relative to the workspace root.
const WORKING_DIRECTORY: &str = "elysia";

/// How long a usage limit holds her when Claude's rate-limit event names no reset time.
/// Claude's windows reset within five hours; an hour retries sooner without hammering a spent
/// credential.
const DEFAULT_USAGE_HOLD: TimeDelta = TimeDelta::hours(1);

/// One turn as the dispatcher asks for it.
#[derive(Debug, Clone)]
pub struct ElysiaTurn {
    pub turn_id: Uuid,
    pub prompt: String,
    /// Her standing instructions, appended to Claude Code's own system prompt.
    pub instructions: String,
    /// The session the previous turn left, so she carries context. `None` starts fresh.
    pub resume: Option<String>,
}

/// How a turn ended, in Elysia's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ElysiaOutcome {
    /// She finished. `result` is her final text.
    Completed {
        result: Option<String>,
        session_id: Option<String>,
    },
    /// Every credential is spent. The turn is queued again for after `resets_at`.
    UsageLimited {
        resets_at: DateTime<Utc>,
        session_id: Option<String>,
    },
    /// It ended without finishing: an error, a dead process, a cancellation, or a shutdown.
    Failed {
        error: String,
        session_id: Option<String>,
    },
}

/// Why no turn could be run at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RunnerError {
    #[error("the runner refused the turn: {0}")]
    Refused(String),
}

/// What runs Elysia's turns.
#[derive(Debug, Clone)]
pub enum TurnRunner {
    /// The host runner in Elysium's workspace container.
    Host(runner::Runner),
    /// Plays back prepared outcomes, in order, for tests.
    #[cfg(test)]
    Scripted(std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<ElysiaOutcome>>>),
}

impl TurnRunner {
    /// Runs `turn` to its end.
    ///
    /// # Errors
    /// Returns [`RunnerError`] when the runner refuses the turn, such as while it shuts down.
    pub async fn run(&self, turn: ElysiaTurn) -> Result<ElysiaOutcome, RunnerError> {
        match self {
            Self::Host(runner) => run_on_host(runner, turn).await,
            #[cfg(test)]
            Self::Scripted(outcomes) => {
                // Yield once, as a real runner would, so tests see the turn running.
                tokio::task::yield_now().await;
                outcomes
                    .lock()
                    .expect("the script lock is never poisoned")
                    .pop_front()
                    .ok_or_else(|| RunnerError::Refused("the script ran out".to_owned()))
            }
        }
    }
}

async fn run_on_host(
    runner: &runner::Runner,
    turn: ElysiaTurn,
) -> Result<ElysiaOutcome, RunnerError> {
    let working_directory = WorkspacePath::new(WORKING_DIRECTORY)
        .map_err(|error| RunnerError::Refused(error.to_string()))?;
    let resume = turn
        .resume
        .as_deref()
        .and_then(|session_id| session_id.parse::<HarnessSessionId>().ok());

    let mut ticket = runner
        .submit(TurnRequest {
            conversation: ConversationId::new(ELYSIA_USER_ID),
            label: format!("elysia:{}", turn.turn_id),
            prompt: turn.prompt,
            working_directory,
            resume,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: Some(turn.instructions),
            environment: Vec::new(),
            model: None,
            mode: TurnMode::Agent,
        })
        .map_err(|error| RunnerError::Refused(error.to_string()))?;

    // The last rate-limit report says when a spent window resets.
    let mut last_rate_limit: Option<Value> = None;
    while let Some(runner_event) = ticket.events.recv().await {
        match runner_event {
            RunnerEvent::RateLimit { info } => last_rate_limit = Some(info),
            RunnerEvent::Finished(outcome) => {
                return Ok(read_outcome(outcome, last_rate_limit.as_ref(), Utc::now()));
            }
            _ => {}
        }
    }
    event!(
        name: "elysia.turn.unfinished",
        Level::ERROR,
        elysia.turn.id = %turn.turn_id,
        "the runner closed the turn's events without finishing it",
    );
    Err(RunnerError::Refused(
        "the runner closed the turn without finishing it".to_owned(),
    ))
}

/// The runner's outcome in Elysia's terms. `rate_limit` is the turn's last rate-limit
/// report, which names when a spent window resets.
pub fn read_outcome(
    outcome: TurnOutcome,
    rate_limit: Option<&Value>,
    now: DateTime<Utc>,
) -> ElysiaOutcome {
    let session_id = outcome
        .harness_session_id
        .map(|session_id| session_id.to_string());
    match outcome.status {
        runner::TurnStatus::Completed => ElysiaOutcome::Completed {
            result: outcome.result,
            session_id,
        },
        runner::TurnStatus::UsageLimited => ElysiaOutcome::UsageLimited {
            resets_at: rate_limit
                .and_then(resets_at)
                .filter(|resets_at| *resets_at > now)
                .unwrap_or(now + DEFAULT_USAGE_HOLD),
            session_id,
        },
        status => ElysiaOutcome::Failed {
            error: outcome
                .error
                .unwrap_or_else(|| format!("the turn ended as {status:?}")),
            session_id,
        },
    }
}

/// When a rate-limit report says its window resets: `resetsAt`, in Unix seconds, either at
/// the top level or inside `rate_limit_info`, as Claude Code's stream-json writes it.
fn resets_at(info: &Value) -> Option<DateTime<Utc>> {
    let seconds = info
        .get("resetsAt")
        .or_else(|| info.get("rate_limit_info")?.get("resetsAt"))?
        .as_i64()?;
    DateTime::from_timestamp(seconds, 0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn outcome(status: runner::TurnStatus) -> TurnOutcome {
        TurnOutcome {
            status,
            result: Some("Done.\nSummary: Did it.".to_owned()),
            error: None,
            harness_session_id: "session-7".parse().ok(),
            cost_usd_micros: None,
            input_tokens: None,
            output_tokens: None,
        }
    }

    #[test]
    fn a_completed_turn_keeps_its_text_and_session() {
        assert_eq!(
            read_outcome(outcome(runner::TurnStatus::Completed), None, Utc::now()),
            ElysiaOutcome::Completed {
                result: Some("Done.\nSummary: Did it.".to_owned()),
                session_id: Some("session-7".to_owned()),
            }
        );
    }

    #[test]
    fn a_usage_limit_resumes_when_its_window_resets() {
        let now = Utc::now();
        let reset = now + TimeDelta::hours(3);
        let info =
            json!({ "rate_limit_info": { "status": "rejected", "resetsAt": reset.timestamp() } });

        let ElysiaOutcome::UsageLimited { resets_at, .. } =
            read_outcome(outcome(runner::TurnStatus::UsageLimited), Some(&info), now)
        else {
            panic!("a usage-limited outcome");
        };
        assert_eq!(resets_at.timestamp(), reset.timestamp());
    }

    #[test]
    fn a_usage_limit_without_a_reset_time_holds_for_an_hour() {
        let now = Utc::now();
        let ElysiaOutcome::UsageLimited { resets_at, .. } =
            read_outcome(outcome(runner::TurnStatus::UsageLimited), None, now)
        else {
            panic!("a usage-limited outcome");
        };
        assert_eq!(resets_at, now + DEFAULT_USAGE_HOLD);
    }

    #[test]
    fn every_other_end_is_a_failure_with_its_error() {
        let mut interrupted = outcome(runner::TurnStatus::Interrupted);
        interrupted.error = Some("heartbeat timed out".to_owned());
        assert_eq!(
            read_outcome(interrupted, None, Utc::now()),
            ElysiaOutcome::Failed {
                error: "heartbeat timed out".to_owned(),
                session_id: Some("session-7".to_owned()),
            }
        );
    }
}
