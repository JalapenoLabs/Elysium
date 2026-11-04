// Copyright © 2026 Jalapeno Labs

//! The engine's answerer: writes the answer to every question asked of the knowledge base.
//!
//! The API records each ask with the passages its search offered and signals
//! [`Signal::KnowledgeAsks`]; the answerer takes queued asks, oldest first, at most
//! [`MAX_CONCURRENT_ASKS`] at once, and answers each in one text turn on the runner, on the model
//! the `knowledge_ask` agent resolves to (`crate::agent_models`, the triage kind's unless an admin
//! chose another). The prompt and the checks on the answer are `super::ask`. Each ask is
//! announced as it is taken and as it ends (`knowledgeAsk.changed`), so the asker's page and an
//! MCP client waiting on it hear at once.
//!
//! It is a loop of the engine's own rather than a workflow: a run is one workflow on one source,
//! shown to every member on the Runs page, while an ask is its asker's alone, answered in seconds,
//! with nothing a person would retry, skip, or resume; one that fails says why and is asked again.
//!
//! It holds no activity of its own: the runner holds one for every turn (`crate::activity`), and
//! loading passages and checking the answer around it take milliseconds. An ask that does not
//! end within [`ANSWER_TIMEOUT`] is cancelled and fails. An engine that stops kills its turns,
//! and each ask in flight fails saying so; one an engine left running when it died is failed as
//! the next one starts, since nothing is writing its answer any more.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};

use super::ask;
use crate::agent_models::{self, Agent};
use crate::database::Pool;
use crate::models::knowledge_ask::{self, Answer, KnowledgeAsk};
use crate::models::model_setting::ModelChoice;
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::runner::{
    ConversationId, Runner, RunnerEvent, TurnMode, TurnOutcome, TurnRequest, TurnStatus,
    TurnTicket, WorkspacePath,
};

/// How many asks are answered at once. Each is one of the runner's eight turn slots, which
/// sessions and Elysia share, so asks never take more than a few of them.
pub const MAX_CONCURRENT_ASKS: usize = 3;

/// The longest an answer may take. A text turn over a page of passages answers in well under a
/// minute even at high effort; one still going after this is stuck, and the asker is better told
/// to ask again than left waiting.
pub const ANSWER_TIMEOUT: Duration = Duration::from_mins(3);

/// How often the queue is looked at without a signal, in case one was lost.
const TICK: Duration = Duration::from_secs(30);

/// Where answer turns run. They use no tools, so the directory only needs to exist.
const ANSWER_DIRECTORY: &str = ".elysium/asks";

/// The error an ask records when Elysium stopped while its answer was written.
const INTERRUPTED_ERROR: &str = "Elysium stopped while this answer was being written; ask again";

/// Answers the knowledge base's asks. Cheap to clone; clones share everything.
#[derive(Clone)]
pub struct Answerer {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    runner: Runner,
}

impl std::fmt::Debug for Answerer {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("Answerer").finish_non_exhaustive()
    }
}

/// What an [`Answerer`] is built from.
#[derive(Clone)]
pub struct AnswererDeps {
    pub database: Pool,
    pub events: EventBus,
    pub runner: Runner,
}

impl Answerer {
    pub fn new(deps: AnswererDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                runner: deps.runner,
            }),
        }
    }

    /// Answers asks until `shutdown`, then waits for the answers in flight to record how they
    /// ended: the runner stops their turns as the engine stops.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let answerer = self.clone();
        tokio::spawn(async move {
            answerer.fail_interrupted().await;
            // Taken before the first look at the queue, so an ask made during it wakes the next.
            let mut wake = answerer
                .inner
                .events
                .wake_on(|signal| *signal == Signal::KnowledgeAsks);
            let mut answering = JoinSet::new();
            loop {
                answerer.take_what_fits(&mut answering).await;
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = wake.woken() => {}
                    Some(joined) = answering.join_next(), if !answering.is_empty() => {
                        log_if_panicked(joined);
                    }
                    () = tokio::time::sleep(TICK) => {}
                }
            }
            while let Some(joined) = answering.join_next().await {
                log_if_panicked(joined);
            }
        })
    }

    /// Fails the asks an engine left running when it stopped.
    async fn fail_interrupted(&self) {
        let failed = match self.inner.database.get().await {
            Ok(mut connection) => knowledge_ask::fail_running(&mut connection, INTERRUPTED_ERROR)
                .await
                .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        match failed {
            Ok(failed) => {
                for ask in &failed {
                    self.announce(ask);
                }
            }
            Err(message) => event!(
                name: "knowledge.asks.recover.failure",
                Level::ERROR,
                error.message = %message,
                "could not fail the asks left running by the last stop; they stay running",
            ),
        }
    }

    /// Takes as many queued asks as there is room for, and starts answering each.
    async fn take_what_fits(&self, answering: &mut JoinSet<()>) {
        let room = MAX_CONCURRENT_ASKS.saturating_sub(answering.len());
        if room == 0 {
            return;
        }
        let taken = match self.inner.database.get().await {
            Ok(mut connection) => knowledge_ask::take_queued(
                &mut connection,
                i64::try_from(room).expect("a handful of slots"),
            )
            .await
            .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        match taken {
            Ok(asks) => {
                for ask in asks {
                    self.announce(&ask);
                    let answerer = self.clone();
                    answering.spawn(async move { answerer.answer(ask).await });
                }
            }
            Err(message) => event!(
                name: "knowledge.asks.take.failure",
                Level::ERROR,
                error.message = %message,
                "could not take the queued asks; the answerer tries again on its next look",
            ),
        }
    }

    /// Writes `ask`'s answer, or why there is none, and announces how it ended.
    async fn answer(&self, ask: KnowledgeAsk) {
        let written = self.write(&ask).await;
        let recorded = match self.inner.database.get().await {
            Ok(mut connection) => match &written {
                Ok(answer) => knowledge_ask::finish(&mut connection, ask.id, answer).await,
                Err(reason) => knowledge_ask::fail(&mut connection, ask.id, reason).await,
            }
            .map_err(|error| error.to_string()),
            Err(error) => Err(error.to_string()),
        };
        match recorded {
            Ok(Some(ended)) => {
                event!(
                    name: "knowledge.asks.ended",
                    Level::INFO,
                    knowledge.ask.id = %ended.id,
                    knowledge.ask.state = ?ended.state,
                    knowledge.ask.claims = written.as_ref().map_or(0, |answer| answer.claims.len()),
                    "an ask ended {{knowledge.ask.state}}",
                );
                self.announce(&ended);
            }
            Ok(None) => event!(
                name: "knowledge.asks.already_ended",
                Level::DEBUG,
                knowledge.ask.id = %ask.id,
                "an ask had ended before its answer was recorded",
            ),
            Err(message) => event!(
                name: "knowledge.asks.record.failure",
                Level::ERROR,
                knowledge.ask.id = %ask.id,
                error.message = %message,
                "an ask's answer could not be recorded; it is failed as the next engine starts",
            ),
        }
    }

    /// `ask`'s checked answer, or the reason, as its asker reads it, that there is none.
    async fn write(&self, ask: &KnowledgeAsk) -> Result<Answer, String> {
        let (offered, model) = {
            let mut connection =
                self.inner.database.get().await.map_err(|_error| {
                    "The knowledge base could not be read; ask again".to_owned()
                })?;
            let offered = knowledge_ask::offered_passages(&mut connection, &ask.offered())
                .await
                .map_err(|_error| "The knowledge base could not be read; ask again".to_owned())?;
            // Read as each answer starts, so a change applies from the next one.
            let model = agent_models::for_agent(&mut connection, Agent::KnowledgeAsk, None)
                .await
                .map_err(|_error| "The model to answer with could not be read".to_owned())?;
            (offered, model.choice)
        };
        // Everything the search offered left the index since: there is nothing left to cite.
        if offered.is_empty() {
            return Ok(Answer::default());
        }

        let prompt = ask::prompt(&ask.question, &offered);
        let included = &offered[..prompt.passages_included];
        let reply = self.ask_model(ask, prompt.text, model).await?;
        ask::check_answer(&reply, included)
            .map_err(|error| format!("The answer could not be read ({error}); ask again"))
    }

    /// The model's reply to `prompt`, from a text turn under `ask`'s id, cancelled past
    /// [`ANSWER_TIMEOUT`].
    async fn ask_model(
        &self,
        ask: &KnowledgeAsk,
        prompt: String,
        model: ModelChoice,
    ) -> Result<String, String> {
        let request = TurnRequest {
            conversation: ConversationId::new(ask.id),
            label: format!("knowledge_ask:{}", ask.id),
            prompt,
            working_directory: WorkspacePath::new(ANSWER_DIRECTORY)
                .expect("the answer directory is a plain relative path"),
            resume: None,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: Some(model.model),
            effort: Some(model.effort),
            mode: TurnMode::Text {
                system_prompt: ask::SYSTEM_PROMPT.to_owned(),
            },
        };
        let runner = &self.inner.runner;
        let mut ticket = runner
            .submit_as(ask.id, request)
            .map_err(|_shutting_down| INTERRUPTED_ERROR.to_owned())?;
        let outcome = match tokio::time::timeout(ANSWER_TIMEOUT, finished(&mut ticket)).await {
            Ok(outcome) => outcome,
            Err(_elapsed) => {
                runner.cancel(ask.id);
                // Drained, so the turn is gone before the ask says so.
                finished(&mut ticket).await;
                return Err(format!(
                    "No answer was written within {} minutes; ask again",
                    ANSWER_TIMEOUT.as_secs() / 60
                ));
            }
        };
        let outcome =
            outcome.ok_or_else(|| "The answer's turn ended without an outcome".to_owned())?;
        reply_of(outcome)
    }

    fn announce(&self, ask: &KnowledgeAsk) {
        self.inner
            .events
            .publish(&ServerEvent::KnowledgeAskChanged {
                id: ask.id,
                asked_by: ask.created_by,
                state: ask.state,
            });
    }
}

/// How a turn ended, once it has: `None` when its events stopped without saying.
async fn finished(ticket: &mut TurnTicket) -> Option<TurnOutcome> {
    while let Some(runner_event) = ticket.events.recv().await {
        if let RunnerEvent::Finished(outcome) = runner_event {
            return Some(outcome);
        }
    }
    None
}

/// The model's reply from a turn that ended so, or why there is none, as the asker reads it.
fn reply_of(outcome: TurnOutcome) -> Result<String, String> {
    match outcome.status {
        TurnStatus::Completed => Ok(outcome.result.unwrap_or_default()),
        TurnStatus::UsageLimited => {
            Err("Elysium's Claude usage is spent for now; ask again once it resets".to_owned())
        }
        TurnStatus::Cancelled | TurnStatus::Interrupted => Err(INTERRUPTED_ERROR.to_owned()),
        TurnStatus::Failed => Err(outcome
            .error
            .map(|error| error.trim().to_owned())
            .filter(|error| !error.is_empty())
            .map_or_else(
                || "The answer could not be written".to_owned(),
                |error| format!("The answer could not be written: {error}"),
            )),
    }
}

fn log_if_panicked(joined: Result<(), tokio::task::JoinError>) {
    if let Err(error) = joined {
        event!(
            name: "knowledge.asks.answer.panicked",
            Level::ERROR,
            error.message = %error,
            "answering an ask panicked; it is failed as the next engine starts",
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(status: TurnStatus, result: Option<&str>, error: Option<&str>) -> TurnOutcome {
        TurnOutcome {
            status,
            result: result.map(str::to_owned),
            error: error.map(str::to_owned),
            harness_session_id: None,
            cost_usd_micros: None,
            input_tokens: None,
            output_tokens: None,
            usage_resets_at: None,
        }
    }

    #[test]
    fn a_completed_turn_answers_its_text_and_every_other_says_why_not() {
        assert_eq!(
            reply_of(outcome(TurnStatus::Completed, Some("{}"), None)),
            Ok("{}".to_owned())
        );
        let spent = reply_of(outcome(TurnStatus::UsageLimited, None, Some("limit"))).unwrap_err();
        assert!(spent.contains("usage is spent"), "{spent}");
        for stopped in [TurnStatus::Cancelled, TurnStatus::Interrupted] {
            assert_eq!(
                reply_of(outcome(stopped, None, None)),
                Err(INTERRUPTED_ERROR.to_owned())
            );
        }
        assert_eq!(
            reply_of(outcome(TurnStatus::Failed, None, Some(" Not logged in "))),
            Err("The answer could not be written: Not logged in".to_owned())
        );
        assert_eq!(
            reply_of(outcome(TurnStatus::Failed, None, Some("  "))),
            Err("The answer could not be written".to_owned())
        );
    }
}
