// Copyright © 2026 Jalapeno Labs

//! The request workflow's last step, [`HandleRequest`]: the conversation's agent reads its
//! messages and raises an action item for each question or request the person owes.
//!
//! # One session per conversation
//!
//! A conversation's agent is one Claude session, kept on its row (`harness_session_id`) and
//! resumed by every turn of every run, so it remembers what it already raised and what the person
//! already answered. Its turns are text in, text out (`TurnMode::ResumableText`): it can neither
//! send nor change anything, and what it answers is checked (`super::handling`).
//!
//! # Turns
//!
//! The step gives the agent every message of the conversation it has not been given, oldest
//! first, then looks again: a message that arrived while the turn ran is given to the same
//! session in the next turn, not to a new run. It ends when nothing is left to give, after at most
//! [`MAX_TURNS`] turns, so a conversation that never stops holds a worker slot for a bounded time
//! and its newest messages wait for the next run.
//!
//! # Waiting
//!
//! When the newest message reads as unfinished the agent may ask to wait, once per run: the step
//! records [`handling::WAIT`]'s end on the conversation and waits it out, then gives the agent
//! whatever arrived meanwhile and asks again. The end is a row, not a timer, so an attempt that
//! takes over after its engine stopped waits out only what is left of it.
//!
//! # Doing it once
//!
//! Each turn's messages are marked given, its session stored, its wait recorded, and the items it
//! raised created in one transaction. An attempt that takes over finds every message an earlier
//! turn landed already given, so no message is given twice and no item raised twice; a turn cut
//! short before it landed is asked again, resuming the session as it was.
//!
//! # What it raises
//!
//! Each item is an inbox item raised by Elysia (`elysia`), owned by the person: a question or
//! request is something to triage, which only the person accepts. It names what the person owes,
//! quotes the messages it came from with a link to the last, and joins the projects segmenting
//! attributed those messages to. Nothing is sent: answering is the person's own act
//! (`crate::drafts`).
//!
//! # What it logs
//!
//! Every company member can read a run's log, and a person's messages are theirs alone, so the
//! log carries counts and ids, never a title or a word said.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::sync::Arc;

use anyhow::anyhow;
use chrono::Utc;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::handling::{self, Answer, Item, Prompt, PromptMessage};
use crate::action_items::Actor;
use crate::agent_models::AgentKind;
use crate::database::Pool;
use crate::meetings::segment::{Segmented, SegmentedTopic};
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::conversation::{self, Conversation, TurnRecorded};
use crate::models::project;
use crate::models::timeline_record::{self, TimelineRecord};
use crate::models::user;
use crate::realtime::EventBus;
use crate::routes::v1::action_items::publish_item_writes;
use crate::runner::events::{HarnessSessionId, RunnerEvent, TurnStatus};
use crate::runner::{ConversationId, Runner, TurnMode, TurnRequest, WorkspacePath};
use crate::workflows::{Done, Step, StepContext, StepFailure, StepLog};

/// Where handling turns run. They use no tools, but Claude Code finds a session again by its
/// directory, so every turn of every conversation runs in this one.
const HANDLING_DIRECTORY: &str = ".elysium/conversations";

/// The most turns one run gives its agent. A chat that keeps going past it is carried on by the
/// next run, in the same session, so a busy conversation never holds a worker slot indefinitely.
pub const MAX_TURNS: usize = 6;

/// The most messages one turn gives the agent; more wait for its next turn.
const MESSAGES_PER_TURN: i64 = 50;

/// How sure segmenting must be of a project for an item to join it.
const ITEM_PROJECT_CONFIDENCE: f32 = 0.5;

/// How much of each message an item quotes.
const QUOTE_CHARACTERS: usize = 500;

/// The longest notes an item keeps, inside `action_items_notes_length`.
const ITEM_NOTES_CHARACTERS: usize = 10_000;

/// What [`HandleRequest`] gives: what its agent read and raised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Handled {
    pub conversation_id: Uuid,
    /// How many turns the agent took, and how many messages it was given in them.
    pub turns: usize,
    pub messages: usize,
    /// The inbox items it raised, in order.
    pub action_item_ids: Vec<Uuid>,
}

/// Hands a conversation's messages to its agent. See the module's docs.
#[derive(Clone)]
pub struct HandleRequest {
    inner: Arc<Inner>,
}

struct Inner {
    database: Pool,
    events: EventBus,
    runner: Runner,
}

impl std::fmt::Debug for HandleRequest {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HandleRequest")
            .finish_non_exhaustive()
    }
}

/// What [`HandleRequest`] is built from.
#[derive(Clone)]
pub struct HandleRequestDeps {
    pub database: Pool,
    pub events: EventBus,
    pub runner: Runner,
}

/// What every turn of one attempt shares: the conversation, whose it is, and what segmenting
/// found.
struct Reading<'input> {
    conversation: Conversation,
    person: String,
    topics: &'input [SegmentedTopic],
    project_names: Vec<(Uuid, String)>,
}

/// What one turn decided, once landed.
struct Landed {
    delivered: usize,
    waits: bool,
    raised: Vec<Recorded<ActionItem>>,
}

impl HandleRequest {
    pub fn new(deps: HandleRequestDeps) -> Self {
        Self {
            inner: Arc::new(Inner {
                database: deps.database,
                events: deps.events,
                runner: deps.runner,
            }),
        }
    }

    async fn connection(
        &self,
    ) -> Result<diesel_async::pooled_connection::deadpool::Object<AsyncPgConnection>, StepFailure>
    {
        self.inner.database.get().await.map_err(|error| {
            StepFailure::retryable(format!("No database connection available: {error}"))
        })
    }

    /// The conversation `input`'s records were gathered in, whose it is, and the names of the
    /// projects segmenting attributed them to.
    async fn reading<'input>(
        &self,
        input: &'input Segmented,
    ) -> Result<Option<Reading<'input>>, StepFailure> {
        let mut record_ids: Vec<Uuid> = input.record_ids.clone();
        for topic in &input.segments {
            record_ids.extend_from_slice(&topic.record_ids);
        }
        let mut connection = self.connection().await?;
        let Some(found) = conversation::of_records(&mut connection, &record_ids)
            .await
            .map_err(StepFailure::retryable)?
        else {
            return Ok(None);
        };
        let person = user::find(&mut connection, found.created_by)
            .await
            .map_err(StepFailure::retryable)?
            .name;
        let project_names = project::list(&mut connection)
            .await
            .map_err(StepFailure::retryable)?
            .into_iter()
            .map(|project| (project.id, project.name))
            .collect();
        Ok(Some(Reading {
            conversation: found,
            person,
            topics: &input.segments,
            project_names,
        }))
    }

    /// One turn, if one is owed: waits out a wait the conversation holds, gives the agent what it
    /// has not been given, and lands what it decided. `None` when nothing is left to give.
    async fn turn(
        &self,
        reading: &Reading<'_>,
        first_of_run: bool,
        context: &StepContext,
    ) -> Result<Option<Landed>, StepFailure> {
        let log = &context.log;
        let current = {
            let mut connection = self.connection().await?;
            conversation::find(&mut connection, reading.conversation.id)
                .await
                .map_err(|error| match error {
                    diesel::result::Error::NotFound => {
                        StepFailure::permanent("The conversation was removed with its connector")
                    }
                    other => StepFailure::retryable(other),
                })?
        };
        let after_wait = current.waiting_until.is_some();
        if let Some(until) = current.waiting_until
            && let Ok(left) = (until - Utc::now()).to_std()
        {
            log.line(&format!(
                "Waiting {} seconds for more, as the agent asked",
                left.as_secs()
            ));
            tokio::time::sleep(left).await;
        }

        let (waiting, records) = {
            let mut connection = self.connection().await?;
            let waiting =
                conversation::undelivered_of(&mut connection, current.id, MESSAGES_PER_TURN)
                    .await
                    .map_err(StepFailure::retryable)?;
            let ids: Vec<Uuid> = waiting
                .iter()
                .map(|message| message.timeline_record_id)
                .collect();
            let mut records = timeline_record::find_many(&mut connection, &ids)
                .await
                .map_err(StepFailure::retryable)?;
            records.sort_by_key(|record| (record.occurred_at, record.id));
            (ids, records)
        };
        if waiting.is_empty() && !after_wait {
            return Ok(None);
        }

        let may_wait = current.waited_in_run_id != Some(context.run_id);
        let messages: Vec<PromptMessage<'_>> = records
            .iter()
            .map(|record| PromptMessage {
                occurred_at: record.occurred_at,
                actor: &record.actor,
                text: if record.body.trim().is_empty() {
                    &record.title
                } else {
                    &record.body
                },
            })
            .collect();
        let topics: &[SegmentedTopic] = if first_of_run { reading.topics } else { &[] };
        let prompt = handling::prompt(&Prompt {
            title: &current.title,
            person: &reading.person,
            new_session: current.harness_session_id.is_none(),
            topics,
            project_names: &reading.project_names,
            messages: &messages,
            after_wait,
            may_wait,
        });
        let (answer, session) = self.ask(&current, prompt, context).await?;
        let answer = handling::parse_answer(&answer, records.len())
            .map_err(|error| StepFailure::retryable(format!("The agent's answer: {error}")))?;

        let waits = answer == Answer::Wait && may_wait;
        if answer == Answer::Wait && !may_wait {
            log.line("The agent asked to wait again, which a run allows once; it raised nothing");
        }
        let items = match answer {
            Answer::Items(items) => items,
            Answer::Wait => Vec::new(),
        };
        let new_items: Vec<NewActionItem> = items
            .iter()
            .map(|item| item_for(&current, item, &records, reading.topics))
            .collect();
        let raised = self
            .land(&current, &waiting, session, waits, new_items, context)
            .await?;
        Ok(Some(Landed {
            delivered: waiting.len(),
            waits,
            raised,
        }))
    }

    /// Lands one turn on `current` in one transaction: its session, the messages it was given,
    /// its wait, and the items it raised, as Elysia.
    async fn land(
        &self,
        current: &Conversation,
        delivered: &[Uuid],
        session: Option<HarnessSessionId>,
        waits: bool,
        new_items: Vec<NewActionItem>,
        context: &StepContext,
    ) -> Result<Vec<Recorded<ActionItem>>, StepFailure> {
        let mut connection = self.connection().await?;
        let now = Utc::now();
        let run_id = context.run_id;
        let conversation_id = current.id;
        connection
            .transaction(async move |connection| {
                conversation::lock(connection, conversation_id).await?;
                let turn = TurnRecorded {
                    harness_session_id: session.map(|session| session.as_str().to_owned()),
                    delivered,
                    wait: waits.then(|| (now + handling::WAIT, run_id)),
                };
                conversation::record_turn(connection, conversation_id, &turn, now).await?;
                let existing: HashSet<Uuid> = project::list(connection)
                    .await?
                    .into_iter()
                    .map(|project| project.id)
                    .collect();
                let mut raised = Vec::with_capacity(new_items.len());
                for mut new_item in new_items {
                    // A project removed since segmenting is left out rather than failing the turn.
                    new_item
                        .project_ids
                        .retain(|project_id| existing.contains(project_id));
                    let created = action_item::create(connection, new_item, Actor::Elysia, now)
                        .await
                        .map_err(|error| anyhow!("cannot raise an inbox item: {error}"))?;
                    raised.push(created);
                }
                Ok::<_, anyhow::Error>(raised)
            })
            .await
            .map_err(StepFailure::retryable)
    }

    /// One turn of `current`'s session, answering the agent's final text and the session to
    /// resume next.
    async fn ask(
        &self,
        current: &Conversation,
        prompt: String,
        context: &StepContext,
    ) -> Result<(String, Option<HarnessSessionId>), StepFailure> {
        // Read as each turn starts, so an admin's change applies from the next one.
        let model = context.model().await?;
        let resume = current
            .harness_session_id
            .as_deref()
            .and_then(|session| session.parse::<HarnessSessionId>().ok());
        let request = TurnRequest {
            // The conversation's own, so the runner never runs two of its turns at once.
            conversation: ConversationId::new(current.id),
            label: format!("conversation:{}", current.id),
            prompt,
            working_directory: WorkspacePath::new(HANDLING_DIRECTORY)
                .expect("the handling directory is a plain relative path"),
            resume,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: Some(model.model),
            effort: Some(model.effort),
            mode: TurnMode::ResumableText {
                system_prompt: handling::SYSTEM_PROMPT.to_owned(),
            },
        };
        let mut ticket = self
            .inner
            .runner
            .submit(request)
            .map_err(|_shutting_down| StepFailure::retryable("Elysium is shutting down"))?;
        let outcome = loop {
            match ticket.events.recv().await {
                Some(RunnerEvent::Finished(outcome)) => break outcome,
                Some(_) => {}
                None => {
                    return Err(StepFailure::retryable(
                        "The handling turn ended without an outcome",
                    ));
                }
            }
        };
        if outcome.status != TurnStatus::Completed {
            return Err(StepFailure::retryable(anyhow!(
                "The handling turn ended {:?}: {}",
                outcome.status,
                outcome.error.unwrap_or_default()
            )));
        }
        Ok((
            outcome.result.unwrap_or_default(),
            outcome.harness_session_id,
        ))
    }

    /// Tells every client of the items a turn raised.
    async fn announce(&self, raised: Vec<Recorded<ActionItem>>, log: &StepLog) {
        if raised.is_empty() {
            return;
        }
        let published = async {
            let mut connection = self.inner.database.get().await?;
            publish_item_writes(&self.inner.events, &mut connection, raised, &[], Utc::now())
                .await
                .map_err(|error| anyhow!("{error}"))?;
            anyhow::Ok(())
        };
        if let Err(error) = published.await {
            // The items are stored; a page that missed them loads them on its next read.
            log.line(&format!(
                "The raised items could not be announced: {error:#}"
            ));
        }
    }
}

impl Step for HandleRequest {
    const NAME: &'static str = "handle_request";
    /// It decides what a conversation asks of its person in an agent turn.
    const AGENT_KIND: Option<AgentKind> = Some(AgentKind::Triage);
    type Input = Segmented;
    type Output = Handled;

    async fn run(
        &self,
        input: Segmented,
        context: StepContext,
    ) -> Result<Done<Handled>, StepFailure> {
        let log = &context.log;
        let Some(reading) = self.reading(&input).await? else {
            return Ok(Done::Skipped {
                reason: "No conversation gathered these messages, so nothing to handle".to_owned(),
            });
        };
        let mut handled = Handled {
            conversation_id: reading.conversation.id,
            turns: 0,
            messages: 0,
            action_item_ids: Vec::new(),
        };
        while handled.turns < MAX_TURNS {
            let first_of_run = handled.turns == 0;
            let Some(landed) = self.turn(&reading, first_of_run, &context).await? else {
                break;
            };
            handled.turns += 1;
            handled.messages += landed.delivered;
            log.line(&format!(
                "Turn {}: gave the agent {} messages; {}",
                handled.turns,
                landed.delivered,
                if landed.waits {
                    "it asked to wait for more".to_owned()
                } else {
                    format!("it raised {} items", landed.raised.len())
                }
            ));
            handled
                .action_item_ids
                .extend(landed.raised.iter().map(|item| item.record.id));
            self.announce(landed.raised, log).await;
        }
        if handled.turns == MAX_TURNS {
            log.line(&format!(
                "Stopped after {MAX_TURNS} turns; anything newer is carried on by the next run"
            ));
        }
        if handled.turns == 0 {
            return Ok(Done::Skipped {
                reason: "Every message was already given to the conversation's agent".to_owned(),
            });
        }
        let summary = summary_of(&handled);
        Ok(Done::Succeeded {
            output: handled,
            summary: Some(summary),
        })
    }
}

/// The one line a handled run's step collapses to.
fn summary_of(handled: &Handled) -> String {
    let counted = |count: usize, one: &str, many: &str| {
        if count == 1 {
            format!("1 {one}")
        } else {
            format!("{count} {many}")
        }
    };
    let read = format!(
        "Read {} in {}",
        counted(handled.messages, "message", "messages"),
        counted(handled.turns, "turn", "turns")
    );
    match handled.action_item_ids.len() {
        0 => format!("{read}; nothing for the person"),
        raised => format!(
            "{read} and raised {} for the person",
            counted(raised, "item", "items")
        ),
    }
}

/// The inbox item `item` raises in `current`: what the agent said the person owes, the messages
/// it came from quoted with a link to the last, in the projects segmenting attributed them to.
fn item_for(
    current: &Conversation,
    item: &Item,
    records: &[TimelineRecord],
    topics: &[SegmentedTopic],
) -> NewActionItem {
    let quoted: Vec<&TimelineRecord> = item
        .messages
        .iter()
        .filter_map(|index| records.get(*index))
        .collect();
    let mut notes = String::new();
    if !item.notes.is_empty() {
        notes.push_str(&item.notes);
        notes.push_str("\n\n");
    }
    let _ = writeln!(notes, "From {}:", current.title);
    for record in &quoted {
        let text = if record.body.trim().is_empty() {
            &record.title
        } else {
            &record.body
        };
        let quote: String = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(QUOTE_CHARACTERS)
            .collect();
        let _ = writeln!(notes, "> {}: {quote}", record.actor);
    }
    if let Some(url) = quoted.last().and_then(|record| record.url.as_deref()) {
        let _ = writeln!(notes, "{url}");
    }

    NewActionItem {
        title: item.title.clone(),
        notes: notes
            .trim_end()
            .chars()
            .take(ITEM_NOTES_CHARACTERS)
            .collect(),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: projects_of(&quoted, topics),
        initiative_ids: Vec::new(),
    }
}

/// The projects segmenting attributed `quoted` to, sure enough to file an item under, sorted and
/// each once, as an item's projects are stored.
fn projects_of(quoted: &[&TimelineRecord], topics: &[SegmentedTopic]) -> Vec<Uuid> {
    let mut by_record: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for topic in topics {
        for record_id in &topic.record_ids {
            let sure = topic
                .projects
                .iter()
                .filter(|project| project.confidence >= ITEM_PROJECT_CONFIDENCE)
                .map(|project| project.project_id);
            by_record.entry(*record_id).or_default().extend(sure);
        }
    }
    let projects: BTreeSet<Uuid> = quoted
        .iter()
        .filter_map(|record| by_record.get(&record.id))
        .flatten()
        .copied()
        .collect();
    projects.into_iter().collect()
}

#[cfg(test)]
impl HandleRequest {
    /// The step over `database`, with a runner whose workspace Docker never answers: enough for a
    /// conversation whose every message its agent was already given.
    pub fn for_test(database: Pool) -> Self {
        let docker = bollard::Docker::connect_with_http(
            "http://127.0.0.1:9",
            5,
            bollard::API_DEFAULT_VERSION,
        )
        .expect("a static address parses");
        let workspace = crate::runner::workspace::WorkspaceContainer::new(
            docker,
            "elysium-workspace:test".to_owned(),
        );
        let runner = Runner::new(
            workspace,
            database.clone(),
            Arc::new(crate::test_support::cipher()),
            crate::activity::Activities::new(),
            crate::claude_accounts::TurnSignal::default(),
            &tokio_util::sync::CancellationToken::new(),
        );
        Self::new(HandleRequestDeps {
            database,
            events: EventBus::in_process(),
            runner,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta};
    use serde_json::json;

    use super::*;
    use crate::meetings::segmentation::SegmentProject;

    fn conversation() -> Conversation {
        Conversation {
            id: Uuid::now_v7(),
            connector_id: Uuid::now_v7(),
            key: "T1/D1".to_owned(),
            title: "Ada in a direct message in Mooreslab".to_owned(),
            harness_session_id: None,
            waiting_until: None,
            waited_in_run_id: None,
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn record(seconds: i64, body: &str, url: Option<&str>) -> TimelineRecord {
        let base: DateTime<Utc> = "2026-11-04T12:00:00Z".parse().expect("a time");
        TimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "slack".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: base + TimeDelta::seconds(seconds),
            actor: "Ada".to_owned(),
            title: "Ada in a direct message in Mooreslab".to_owned(),
            body: body.to_owned(),
            url: url.map(str::to_owned),
            media: None,
            metadata: json!({}),
            action_item_id: None,
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            excerpt_mark_id: None,
            deleted_at: None,
        }
    }

    #[test]
    fn an_item_quotes_its_messages_and_joins_the_projects_they_are_surely_about() {
        let records = [
            record(0, "Hey", None),
            record(
                5,
                "Can you send me\nthe Q3 deck?",
                Some("https://slack.example/p2"),
            ),
        ];
        let (website, maybe) = (Uuid::now_v7(), Uuid::now_v7());
        let topics = [SegmentedTopic {
            title: "Q3 deck".to_owned(),
            summary: "Ada needs the deck.".to_owned(),
            start_ms: 0,
            end_ms: 5_000,
            projects: vec![
                SegmentProject {
                    project_id: website,
                    confidence: 0.8,
                },
                SegmentProject {
                    project_id: maybe,
                    confidence: 0.3,
                },
            ],
            record_ids: vec![records[1].id],
        }];
        let item = Item {
            title: "Send Ada the Q3 deck".to_owned(),
            notes: "Ada needs it for Friday.".to_owned(),
            messages: vec![1],
        };

        let raised = item_for(&conversation(), &item, &records, &topics);
        assert_eq!(raised.title, "Send Ada the Q3 deck");
        assert_eq!(
            raised.notes,
            "Ada needs it for Friday.\n\nFrom Ada in a direct message in Mooreslab:\n> Ada: Can \
             you send me the Q3 deck?\nhttps://slack.example/p2"
        );
        assert_eq!(raised.state, ActionItemState::Inbox);
        assert_eq!(raised.owner, Owner::User);
        assert_eq!(
            raised.project_ids,
            vec![website],
            "only a sure attribution files it"
        );
    }

    #[test]
    fn a_summary_says_what_was_read_and_raised() {
        let mut handled = Handled {
            conversation_id: Uuid::now_v7(),
            turns: 2,
            messages: 5,
            action_item_ids: Vec::new(),
        };
        assert_eq!(
            summary_of(&handled),
            "Read 5 messages in 2 turns; nothing for the person"
        );
        handled.action_item_ids.push(Uuid::now_v7());
        assert_eq!(
            summary_of(&handled),
            "Read 5 messages in 2 turns and raised 1 item for the person"
        );
        handled.turns = 1;
        handled.messages = 1;
        handled.action_item_ids.push(Uuid::now_v7());
        assert_eq!(
            summary_of(&handled),
            "Read 1 message in 1 turn and raised 2 items for the person"
        );
    }
}
