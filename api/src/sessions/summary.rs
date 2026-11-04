// Copyright © 2026 Jalapeno Labs

//! A one or two sentence summary of every finished turn, so a conversation reads like a
//! GitHub Actions log: each turn collapses to its summary and expands to its full events.
//!
//! The follower gathers a [`Digest`] of the turn as its events pass (already scrubbed of the
//! turn's secrets), and once the turn has finished, [`summarize`] asks the model the
//! `turn_summary` setting names (Haiku 5.5 at low effort by default, `crate::agent_models`)
//! for the summary as a text turn on the runner: the official CLI, on the same credentials as
//! every turn, with no tools, no settings, and nothing kept to resume. It runs in a
//! conversation of its own, so it never queues behind the session's next prompt.
//!
//! A summary of a session with a project goes on that project's timeline in the transaction that
//! records it (`super::timeline`), so the project reads its agents' work beside its meetings and
//! messages, and the knowledge base finds it.
//!
//! A summary is a convenience. One that fails is logged and left out, and the conversation
//! shows the turn's prompt in its place; it never touches the turn's own outcome.

use std::fmt::Write as _;

use diesel_async::{AsyncConnection, AsyncPgConnection};
use tracing::{Level, event};
use uuid::Uuid;

use super::queue::TurnQueue;
use super::views::TurnSummaryView;
use crate::agent_models::{self, Agent};
use crate::models::coding_session;
use crate::models::model_setting::ModelChoice;
use crate::models::timeline_record::TimelineRecord;
use crate::models::turn_summary::{self, TurnSummary};
use crate::realtime::ServerEvent;
use crate::runner::{
    ConversationId, RunnerEvent, TurnMode, TurnOutcome, TurnRequest, TurnStatus, WorkspacePath,
};

/// Where summary turns run. They use no tools, so the directory only needs to exist.
const SUMMARY_DIRECTORY: &str = ".elysium/summaries";

/// How much of the prompt, the result, and the error the digest carries. A summary needs the
/// gist; a whole transcript would cost more than the turn it describes.
const PROMPT_CHARACTERS: usize = 2_000;
const RESULT_CHARACTERS: usize = 2_000;
const ERROR_CHARACTERS: usize = 500;
/// How many tool calls the digest lists by name, in order.
const MAX_TOOL_CALLS: usize = 60;
/// The longest summary kept; the column allows 500.
pub const SUMMARY_MAX_CHARACTERS: usize = 400;

const SYSTEM_PROMPT: &str = "\
You summarize one turn of a coding agent's work for a log, the way a CI step is titled. \
Answer with one or two plain sentences, at most 300 characters, in the past tense, saying what \
the agent did and how it ended. No preamble, no markdown, no lists, no quoting the prompt.";

/// What a turn did, gathered as its events pass.
#[derive(Debug, Default)]
pub(super) struct Digest {
    prompt: String,
    tool_calls: Vec<String>,
    omitted_tool_calls: usize,
}

impl Digest {
    pub fn observe(&mut self, runner_event: &RunnerEvent) {
        match runner_event {
            RunnerEvent::TurnStarted { prompt, .. } => {
                self.prompt = truncated(prompt, PROMPT_CHARACTERS);
            }
            RunnerEvent::ToolStarted { tool_name, .. } => {
                if self.tool_calls.len() < MAX_TOOL_CALLS {
                    self.tool_calls.push(tool_name.clone());
                } else {
                    self.omitted_tool_calls += 1;
                }
            }
            _ => {}
        }
    }

    /// The summary turn's prompt.
    fn prompt(&self, outcome: &TurnOutcome) -> String {
        let mut prompt = format!("The user asked:\n{}\n\n", self.prompt);
        if self.tool_calls.is_empty() {
            prompt.push_str("The agent called no tools.\n");
        } else {
            prompt.push_str("The agent called these tools, in order: ");
            prompt.push_str(&self.tool_calls.join(", "));
            if self.omitted_tool_calls > 0 {
                let _ = write!(prompt, ", and {} more", self.omitted_tool_calls);
            }
            prompt.push_str(".\n");
        }
        let ended = match outcome.status {
            TurnStatus::Completed => "completed",
            TurnStatus::Failed => "failed",
            TurnStatus::Cancelled => "was cancelled",
            TurnStatus::Interrupted => "was interrupted",
            TurnStatus::UsageLimited => "ran out of usage",
        };
        let _ = writeln!(prompt, "The turn {ended}.");
        if let Some(result) = &outcome.result {
            prompt.push_str("\nThe agent's final message:\n");
            prompt.push_str(&truncated(result, RESULT_CHARACTERS));
            prompt.push('\n');
        }
        if let Some(error) = &outcome.error {
            prompt.push_str("\nThe error:\n");
            prompt.push_str(&truncated(error, ERROR_CHARACTERS));
            prompt.push('\n');
        }
        prompt
    }
}

/// Whether a turn that ended so gets a summary. A cancelled, interrupted, or usage-limited
/// turn says all there is to say with its state, which the conversation shows translated.
pub(super) const fn deserves_summary(status: TurnStatus) -> bool {
    matches!(status, TurnStatus::Completed | TurnStatus::Failed)
}

/// The summary turn for a turn of `session_id` that ended with `outcome`, on `model`.
fn request(
    session_id: i64,
    digest: &Digest,
    outcome: &TurnOutcome,
    model: ModelChoice,
) -> TurnRequest {
    TurnRequest {
        conversation: ConversationId::new(Uuid::now_v7()),
        label: format!("summary:session:{session_id}"),
        prompt: digest.prompt(outcome),
        working_directory: WorkspacePath::new(SUMMARY_DIRECTORY)
            .expect("the summary directory is a plain relative path"),
        resume: None,
        mcp_servers: Vec::new(),
        allowed_tools: None,
        system_prompt_append: None,
        environment: Vec::new(),
        model: Some(model.model),
        effort: Some(model.effort),
        mode: TurnMode::Text {
            system_prompt: SYSTEM_PROMPT.to_owned(),
        },
    }
}

/// The model's answer as a summary: one line, within [`SUMMARY_MAX_CHARACTERS`], or `None`
/// when nothing is left.
fn cleaned(answer: &str) -> Option<String> {
    let line = answer.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = line.trim_matches('"');
    if line.is_empty() {
        return None;
    }
    Some(truncated(line, SUMMARY_MAX_CHARACTERS))
}

/// `text`'s first `limit` characters, with an ellipsis when it was cut.
fn truncated(text: &str, limit: usize) -> String {
    match text.char_indices().nth(limit) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_owned(),
    }
}

/// Asks for the summary of a turn of `session_id`, which works for `project_id`, records it with
/// its record on the project's timeline, and announces both.
pub(super) async fn summarize(
    queue: &TurnQueue,
    session_id: i64,
    project_id: Option<Uuid>,
    turn_id: Uuid,
    digest: &Digest,
    outcome: &TurnOutcome,
) {
    let failure = |reason: &str| {
        event!(
            name: "sessions.summary.failure",
            Level::WARN,
            session.id = session_id,
            turn.id = %turn_id,
            error.message = %reason,
            "a turn's summary could not be written; the conversation shows its prompt instead",
        );
    };

    let sessions = &queue.inner.sessions;
    // Read as each summary starts, so a change applies from the next one.
    let model = match sessions.inner.database.get().await {
        Ok(mut connection) => {
            agent_models::for_agent(&mut connection, Agent::TurnSummary, project_id)
                .await
                .map(|resolved| resolved.choice)
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    };
    let model = match model {
        Ok(model) => model,
        Err(reason) => {
            failure(&format!("its model setting could not be read: {reason}"));
            return;
        }
    };
    let Ok(mut ticket) = queue
        .inner
        .runner
        .submit(request(session_id, digest, outcome, model))
    else {
        failure("Elysium is shutting down");
        return;
    };
    let summary_outcome = loop {
        match ticket.events.recv().await {
            Some(RunnerEvent::Finished(summary_outcome)) => break summary_outcome,
            Some(_) => {}
            None => {
                failure("the summary turn ended without an outcome");
                return;
            }
        }
    };
    if summary_outcome.status != TurnStatus::Completed {
        let reason = summary_outcome
            .error
            .unwrap_or_else(|| format!("the summary turn ended {:?}", summary_outcome.status));
        failure(&reason);
        return;
    }
    let Some(summary) = summary_outcome.result.as_deref().and_then(cleaned) else {
        failure("the model answered nothing");
        return;
    };

    let recorded = match sessions.inner.database.get().await {
        Ok(mut connection) => keep(&mut connection, session_id, turn_id, &summary)
            .await
            .map_err(|error| error.to_string()),
        Err(error) => Err(error.to_string()),
    };
    let (row, record) = match recorded {
        Ok(recorded) => recorded,
        Err(message) => {
            failure(&message);
            return;
        }
    };
    let events = &sessions.inner.events;
    events.publish(&ServerEvent::TurnSummarized(TurnSummaryView::from(row)));
    if let Some(record) = record {
        events.publish(&ServerEvent::TimelineRecordCreated {
            id: record.id,
            source_kind: record.source_kind,
            created_by: record.created_by,
            action_item_id: None,
            conversation_id: None,
        });
    }
}

/// Records `summary` for turn `turn_id` of session `session_id` and, for a session with a
/// project, its record on the project's timeline, in one transaction, so a summary is never kept
/// without its record or put on the timeline without being kept.
async fn keep(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    turn_id: Uuid,
    summary: &str,
) -> diesel::QueryResult<(TurnSummary, Option<TimelineRecord>)> {
    connection
        .transaction(async |connection| {
            let row = turn_summary::record(connection, session_id, turn_id, summary).await?;
            // Read with the summary, so the record follows the session as it stands now.
            let session = coding_session::find(connection, session_id).await?;
            let record = super::timeline::record(connection, &row, &session).await?;
            Ok((row, record))
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: TurnStatus) -> TurnOutcome {
        TurnOutcome {
            status,
            result: Some("Fixed the login form and pushed the branch.".to_owned()),
            error: None,
            harness_session_id: None,
            cost_usd_micros: None,
            input_tokens: None,
            output_tokens: None,
            usage_resets_at: None,
        }
    }

    fn tool(name: &str) -> RunnerEvent {
        RunnerEvent::ToolStarted {
            tool_call_id: "call".to_owned(),
            tool_name: name.to_owned(),
            input: serde_json::Value::Null,
        }
    }

    fn digest() -> Digest {
        let mut digest = Digest::default();
        digest.observe(&RunnerEvent::TurnStarted {
            turn_id: Uuid::now_v7(),
            prompt: "Fix the login form".to_owned(),
        });
        digest.observe(&tool("Read"));
        digest.observe(&tool("Edit"));
        digest
    }

    #[test]
    fn a_summary_runs_alone_with_no_tools_settings_or_secrets() {
        let model = Agent::TurnSummary.kind().default_choice();
        let request = request(
            12,
            &digest(),
            &outcome(TurnStatus::Completed),
            model.clone(),
        );

        assert!(request.mcp_servers.is_empty());
        assert!(request.environment.is_empty());
        assert!(request.resume.is_none());
        assert_eq!(request.model, Some(model.model), "its setting's model");
        assert_eq!(request.effort, Some(model.effort), "and its effort");
        assert!(matches!(request.mode, TurnMode::Text { .. }));
        // Its own conversation, so it never waits behind the session's next turn.
        assert_ne!(request.conversation, ConversationId::for_session(12));
        assert_ne!(
            request.working_directory.absolute(),
            crate::sessions::Sessions::directory(12).absolute()
        );
    }

    #[test]
    fn the_digest_carries_the_prompt_the_tools_in_order_and_how_the_turn_ended() {
        let prompt = digest().prompt(&outcome(TurnStatus::Completed));

        assert!(prompt.contains("Fix the login form"));
        assert!(prompt.contains("Read, Edit."));
        assert!(prompt.contains("The turn completed."));
        assert!(prompt.contains("Fixed the login form and pushed the branch."));
    }

    #[test]
    fn a_long_turn_is_cut_down_before_it_reaches_the_model() {
        let mut digest = Digest::default();
        digest.observe(&RunnerEvent::TurnStarted {
            turn_id: Uuid::now_v7(),
            prompt: "x".repeat(PROMPT_CHARACTERS * 3),
        });
        for _ in 0..MAX_TOOL_CALLS + 5 {
            digest.observe(&tool("Bash"));
        }
        let prompt = digest.prompt(&outcome(TurnStatus::Failed));

        assert!(prompt.contains("and 5 more"));
        assert!(prompt.len() < PROMPT_CHARACTERS * 2 + RESULT_CHARACTERS);
    }

    #[test]
    fn only_completed_and_failed_turns_are_summarized() {
        assert!(deserves_summary(TurnStatus::Completed));
        assert!(deserves_summary(TurnStatus::Failed));
        assert!(!deserves_summary(TurnStatus::Cancelled));
        assert!(!deserves_summary(TurnStatus::Interrupted));
        assert!(!deserves_summary(TurnStatus::UsageLimited));
    }

    #[test]
    fn an_answer_becomes_one_bounded_line() {
        assert_eq!(
            cleaned("  \"Fixed the form.\n\nPushed it.\"  ").as_deref(),
            Some("Fixed the form. Pushed it.")
        );
        assert_eq!(cleaned("   "), None);
        let long = cleaned(&"word ".repeat(200)).expect("a summary");
        assert_eq!(long.chars().count(), SUMMARY_MAX_CHARACTERS + 1);
        assert!(long.ends_with('…'));
    }

    // #193: a summary of a session with a project lands on the project's timeline with it.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_summary_is_kept_with_its_record_on_the_sessions_project() {
        use crate::models::coding_session::NewCodingSession;
        use crate::models::project::{self, NewProject};
        use crate::test_support::{TEST_PERSON_ID, migrated_database};

        let (_url, mut connection) = migrated_database().await;
        let project = project::create(
            &mut connection,
            &NewProject {
                created_by: TEST_PERSON_ID,
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("a project");
        let id = coding_session::reserve_id(&mut connection)
            .await
            .expect("an id");
        coding_session::create(
            &mut connection,
            &NewCodingSession {
                created_by: TEST_PERSON_ID,
                id,
                project_id: Some(project.id),
                title: "Login form".to_owned(),
                github_credential_id: None,
                action_item_id: None,
                studio_item_id: None,
            },
        )
        .await
        .expect("a session");
        let turn_id = Uuid::now_v7();

        let (row, record) = keep(&mut connection, id, turn_id, "Fixed the login form.")
            .await
            .expect("kept");
        let record = record.expect("on its project");
        assert_eq!(row.summary, record.title);
        assert_eq!(record.occurred_at, row.created_at);
        let attributions =
            crate::models::timeline_record::attributions_of(&mut connection, &[record.id])
                .await
                .expect("read");
        assert_eq!(attributions.len(), 1);
        assert_eq!(attributions[0].project_id, project.id);
    }
}
