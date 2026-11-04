// Copyright © 2026 Jalapeno Labs

//! The seam between the Project Managers' dispatcher and what runs their turns.
//!
//! A turn runs on the host runner (`crate::runner`): the official Claude Code CLI in the
//! workspace container, resuming the project's one session with `claude -p --resume` and
//! relying on Claude Code's own compaction as it grows. Each project is its own runner
//! conversation, so the runner never runs two of one project's turns at once, and works in its
//! own directory, `project-managers/<project id>`, where its files persist between turns.
//!
//! The turn ends the way one of Elysia's does ([`ElysiaOutcome`], read by the same
//! [`read_outcome`]), with its log beside it: what it said and which tools it called, as the
//! project's page shows a turn expanded.
//!
//! [`ManagerRunner`] is an enum rather than a trait object, following the repository's Rust
//! guidelines: the host runner, plus a scripted one for tests.

use chrono::Utc;
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use crate::elysia::runner::{ElysiaOutcome, RunnerError, read_outcome};
use crate::models::model_setting::ModelChoice;
use crate::runner::{
    self, ConversationId, HarnessSessionId, RunnerEvent, TurnMode, TurnRequest, WorkspacePath,
};

/// The most a turn's log keeps, in bytes, newest kept: as much as a step's log keeps, and what
/// `project_manager_turns.log` allows.
pub const LOG_MAX_BYTES: usize = 65_536;

/// The most of one message or tool failure a log line keeps, in characters. A turn's log is a
/// trail to follow, not a transcript; Claude Code keeps the transcript itself.
const LOG_LINE_MAX_CHARACTERS: usize = 2_000;

/// One turn as the dispatcher asks for it.
#[derive(Debug, Clone)]
pub struct ManagerTurn {
    pub project_id: Uuid,
    pub turn_id: Uuid,
    pub prompt: String,
    /// Its standing instructions, appended to Claude Code's own system prompt.
    pub instructions: String,
    /// The session the project's previous turn left; `None` starts its first.
    pub resume: Option<String>,
    /// Its tool servers, under the turn's grant.
    pub mcp_servers: Vec<runner::McpServer>,
    /// The model and effort its setting names for the project, resolved as the turn starts.
    pub model: ModelChoice,
}

/// How a turn ended, and what it did on the way.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    pub outcome: ElysiaOutcome,
    pub log: TurnLog,
}

/// What runs the Project Managers' turns.
#[derive(Debug, Clone)]
pub enum ManagerRunner {
    /// The host runner in Elysium's workspace container.
    Host(runner::Runner),
    /// Plays back prepared outcomes, in order, and keeps each turn it was asked to run.
    #[cfg(test)]
    Scripted(std::sync::Arc<std::sync::Mutex<Script>>),
}

/// What a scripted runner plays back, and what it was asked.
#[cfg(test)]
#[derive(Debug, Default)]
pub struct Script {
    pub outcomes: std::collections::VecDeque<ElysiaOutcome>,
    pub asked: Vec<ManagerTurn>,
}

impl ManagerRunner {
    /// Runs `turn` to its end.
    ///
    /// # Errors
    /// Returns [`RunnerError`] when the runner refuses the turn, such as while it shuts down.
    pub async fn run(&self, turn: ManagerTurn) -> Result<Ran, RunnerError> {
        match self {
            Self::Host(runner) => run_on_host(runner, turn).await,
            #[cfg(test)]
            Self::Scripted(script) => {
                // Yield once, as a real runner would, so tests see the turn running.
                tokio::task::yield_now().await;
                let mut script = script.lock().expect("the script lock is never poisoned");
                script.asked.push(turn);
                let outcome = script
                    .outcomes
                    .pop_front()
                    .ok_or_else(|| RunnerError::Refused("the script ran out".to_owned()))?;
                Ok(Ran {
                    outcome,
                    log: TurnLog::default(),
                })
            }
        }
    }
}

async fn run_on_host(runner: &runner::Runner, turn: ManagerTurn) -> Result<Ran, RunnerError> {
    let turn_id = turn.turn_id;
    let mut ticket = runner
        .submit(request(turn)?)
        .map_err(|error| RunnerError::Refused(error.to_string()))?;

    let mut log = TurnLog::default();
    // The last rate-limit report says when a spent window resets.
    let mut last_rate_limit: Option<Value> = None;
    while let Some(runner_event) = ticket.events.recv().await {
        match runner_event {
            RunnerEvent::RateLimit { info } => last_rate_limit = Some(info),
            RunnerEvent::Finished(outcome) => {
                let outcome = read_outcome(outcome, last_rate_limit.as_ref(), Utc::now());
                return Ok(Ran { outcome, log });
            }
            other => log.record(&other),
        }
    }
    event!(
        name: "project_managers.turn.unfinished",
        Level::ERROR,
        project_manager.turn.id = %turn_id,
        "the runner closed the turn's events without finishing it",
    );
    Err(RunnerError::Refused(
        "the runner closed the turn without finishing it".to_owned(),
    ))
}

/// The runner's request for `turn`: its project's conversation and directory, its standing
/// instructions, and the model and effort its setting names.
fn request(turn: ManagerTurn) -> Result<TurnRequest, RunnerError> {
    let working_directory = WorkspacePath::new(format!("project-managers/{}", turn.project_id))
        .map_err(|error| RunnerError::Refused(error.to_string()))?;
    let resume = turn
        .resume
        .as_deref()
        .and_then(|session_id| session_id.parse::<HarnessSessionId>().ok());
    Ok(TurnRequest {
        conversation: ConversationId::new(turn.project_id),
        label: format!("project_manager:{}", turn.turn_id),
        prompt: turn.prompt,
        working_directory,
        resume,
        mcp_servers: turn.mcp_servers,
        allowed_tools: None,
        system_prompt_append: Some(turn.instructions),
        environment: Vec::new(),
        model: Some(turn.model.model),
        effort: Some(turn.model.effort),
        mode: TurnMode::Agent,
    })
}

/// A turn's log: one line per thing it said or tool it called, the newest [`LOG_MAX_BYTES`]
/// kept.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TurnLog {
    text: String,
}

impl TurnLog {
    /// Adds the line `runner_event` reads as, when it reads as one. Thinking, progress, and lines
    /// the runner did not understand are left to the session's own transcript.
    pub fn record(&mut self, runner_event: &RunnerEvent) {
        let line = match runner_event {
            RunnerEvent::AgentMessage { text } => text.trim().to_owned(),
            RunnerEvent::ToolStarted { tool_name, .. } => format!("Called {tool_name}"),
            RunnerEvent::ToolCompleted {
                tool_name,
                ok: false,
                output_preview,
                ..
            } => format!("{tool_name} failed: {}", output_preview.trim()),
            RunnerEvent::CredentialRolledOver { reason, .. } => {
                format!("Moved to the next Claude credential: {reason}")
            }
            _ => return,
        };
        if line.is_empty() {
            return;
        }
        self.text.extend(line.chars().take(LOG_LINE_MAX_CHARACTERS));
        self.text.push('\n');
        self.keep_newest();
    }

    /// The log as stored.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Drops the oldest lines until the log fits [`LOG_MAX_BYTES`].
    fn keep_newest(&mut self) {
        if self.text.len() <= LOG_MAX_BYTES {
            return;
        }
        let overflow = self.text.len() - LOG_MAX_BYTES;
        // Cut after the first line break past the overflow, so no line is kept half; the
        // longest line is far shorter than the log, so there always is one.
        let cut = self.text[overflow..]
            .find('\n')
            .map_or(self.text.len(), |offset| overflow + offset + 1);
        self.text.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_turn_runs_in_its_projects_own_conversation_and_directory() {
        let project_id = Uuid::now_v7();
        let request = request(ManagerTurn {
            project_id,
            turn_id: Uuid::now_v7(),
            prompt: "Plan the launch.".to_owned(),
            instructions: "You manage the project.".to_owned(),
            resume: Some("session-4".to_owned()),
            mcp_servers: Vec::new(),
            model: ModelChoice {
                model: "claude-sonnet-5-5".to_owned(),
                effort: crate::models::model_setting::Effort::Medium,
            },
        })
        .expect("its directory is a plain relative path");

        assert_eq!(request.conversation, ConversationId::new(project_id));
        assert_eq!(
            request.working_directory,
            WorkspacePath::new(format!("project-managers/{project_id}")).expect("a path")
        );
        assert_eq!(
            request.resume.map(|session| session.to_string()),
            Some("session-4".to_owned()),
            "every turn resumes the project's one session"
        );
        assert_eq!(request.mode, TurnMode::Agent);
        assert_eq!(request.model.as_deref(), Some("claude-sonnet-5-5"));
    }

    #[test]
    fn the_log_keeps_what_it_said_and_called_and_drops_the_rest() {
        let mut log = TurnLog::default();
        log.record(&RunnerEvent::AgentThinking {
            text: "Hmm.".to_owned(),
        });
        log.record(&RunnerEvent::ToolStarted {
            tool_call_id: "1".to_owned(),
            tool_name: "manager_submit_batch".to_owned(),
            input: json!({}),
        });
        log.record(&RunnerEvent::ToolCompleted {
            tool_call_id: "1".to_owned(),
            tool_name: "manager_submit_batch".to_owned(),
            ok: true,
            output_preview: "{}".to_owned(),
        });
        log.record(&RunnerEvent::ToolCompleted {
            tool_call_id: "2".to_owned(),
            tool_name: "work_item".to_owned(),
            ok: false,
            output_preview: "item 4 is not in this project".to_owned(),
        });
        log.record(&RunnerEvent::AgentMessage {
            text: "Submitted the launch batch.\n".to_owned(),
        });

        assert_eq!(
            log.as_str(),
            "Called manager_submit_batch\nwork_item failed: item 4 is not in this project\n\
             Submitted the launch batch.\n"
        );
    }

    #[test]
    fn a_long_log_keeps_its_newest_whole_lines() {
        let mut log = TurnLog::default();
        for number in 0..200 {
            log.record(&RunnerEvent::AgentMessage {
                text: format!("{number} {}", "x".repeat(LOG_LINE_MAX_CHARACTERS)),
            });
        }
        assert!(log.as_str().len() <= LOG_MAX_BYTES);
        assert!(
            log.as_str()
                .starts_with(|first: char| first.is_ascii_digit())
        );
        assert!(log.as_str().ends_with(&format!("{}\n", "x".repeat(10))));
        assert!(log.as_str().contains("199 "), "the newest line is kept");
    }
}
