// Copyright © 2026 Jalapeno Labs

//! Work from every direct message and mention: messages gathered per conversation, a quiet
//! window, and a request run that hands them to the conversation's agent. See `docs/engine.md`,
//! Conversations.
//!
//! # Gathering
//!
//! Ingest reads Slack, Teams, and mail, and each reader names the conversation a message was said
//! in (`crate::connectors::Conversation`). A message that asks something of its person (a direct
//! message, a mention, a reply in a thread they follow, unread mail) joins its conversation in the
//! transaction that records it ([`gather`]). One that does not, such as the person's own reply,
//! joins only a conversation already being worked on, as context; otherwise it starts nothing.
//!
//! # The quiet window
//!
//! A burst of messages is one request, so nothing starts until a conversation has been quiet for
//! [`QUIET_WINDOW`] since the newest message Elysium read. The window is kept in rows, not
//! timers: each message keeps when it was read, and the engine's loop ([`Conversations::start`])
//! looks for conversations whose newest untaken message is old enough ([`quiet`]), so an engine
//! that restarts finds exactly the conversations its predecessor would have.
//!
//! # A run
//!
//! A quiet conversation gets a run of the request workflow (`workflow`), which takes its waiting
//! messages, and reads them with the messages before them for context: segmenting attributes
//! them to projects, and the handling agent decides what they ask of the person
//! (`handle::HandleRequest`). A conversation has one live run at a time. Messages that arrive
//! while it is live wait for no new run: its agent is given them, and the next run reads them for
//! segmenting alone. A failed run holds the conversation's next one back until a person retries
//! or skips its step, as every workflow's live run does.

pub mod handle;
pub mod handling;
pub mod workflow;

use std::collections::HashMap;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use diesel::QueryResult;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use crate::connectors;
use crate::database::Pool;
use crate::models::connector::Connector;
use crate::models::conversation::{
    self, ConversationMessage, KEY_MAX_CHARACTERS, NewConversation, NewMessage,
    TITLE_MAX_CHARACTERS,
};
use crate::models::timeline_record::TimelineRecord;
use crate::models::workflow_run::{self, Settled, WorkflowRunState};
use crate::realtime::{EventBus, Signal};
use crate::workflows;

/// How long a conversation must go without a new message before work starts on it: long enough
/// for someone to finish a thought typed as several messages, short enough that work starts while
/// the request is fresh.
pub const QUIET_WINDOW: TimeDelta = TimeDelta::seconds(10);

/// How often the loop looks without a reason to, in case a signal was lost or a live run ended.
const LOOK_INTERVAL: Duration = Duration::from_secs(30);

/// How soon the loop looks again at a quiet conversation whose live run is still working: once it
/// finishes, what its agent was given still needs its own run's segmenting.
const BUSY_RECHECK: Duration = Duration::from_secs(5);

/// The most waiting messages one run takes; a larger burst is taken by the runs after it.
const MESSAGES_PER_RUN: i64 = 50;

/// How many earlier messages a run reads again besides what it takes, so segmenting sees what
/// the new messages answer.
const CONTEXT_MESSAGES: i64 = 20;

/// A message ingest is storing, as [`gather`] reads it.
#[derive(Debug, Clone, Copy)]
pub struct Message<'read> {
    pub record: &'read TimelineRecord,
    pub conversation: &'read connectors::Conversation,
    /// Whether it asks something of its person.
    pub wakes: bool,
}

/// Where a message was gathered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Gathered {
    pub conversation_id: Uuid,
    /// Whether it starts work, so the loop is told to look.
    pub wakes: bool,
}

/// Gathers `message` into its conversation of `connector`'s, when it joins one: always when it
/// wakes, otherwise only while the conversation is being worked on (a message waits in it, or a
/// run of it is live). Call it in the transaction that stores the message's record.
///
/// # Errors
/// Propagates any database error.
pub async fn gather(
    connection: &mut AsyncPgConnection,
    connector: &Connector,
    message: Message<'_>,
    now: DateTime<Utc>,
) -> QueryResult<Option<Gathered>> {
    let key: String = message
        .conversation
        .key
        .chars()
        .take(KEY_MAX_CHARACTERS)
        .collect();
    if key.is_empty() {
        event!(
            name: "conversations.gather.unnamed",
            Level::DEBUG,
            timeline_record.id = %message.record.id,
            "a message names no conversation, so it gathers nowhere",
        );
        return Ok(None);
    }

    let found = if message.wakes {
        let title = conversation_title(message);
        let new = NewConversation {
            connector_id: connector.id,
            key: &key,
            title: &title,
            created_by: connector.created_by,
        };
        Some(conversation::upsert(connection, &new, now).await?)
    } else {
        match conversation::find_by_key(connection, connector.id, &key).await? {
            Some(found) if is_being_worked_on(connection, found.id).await? => Some(found),
            _ => None,
        }
    };
    let Some(joined) = found else {
        return Ok(None);
    };
    conversation::add_message(
        connection,
        NewMessage {
            timeline_record_id: message.record.id,
            conversation_id: joined.id,
            wakes: message.wakes,
            received_at: now,
        },
    )
    .await?;
    Ok(Some(Gathered {
        conversation_id: joined.id,
        wakes: message.wakes,
    }))
}

/// What a conversation is called, as `message` names it, or by its sender when the reader had
/// no name for it, as a Teams chat with no topic may not.
fn conversation_title(message: Message<'_>) -> String {
    let named = message.conversation.title.trim();
    let title = if !named.is_empty() {
        named.to_owned()
    } else if message.record.actor.trim().is_empty() {
        "A conversation".to_owned()
    } else {
        format!("A conversation with {}", message.record.actor.trim())
    };
    title.chars().take(TITLE_MAX_CHARACTERS).collect()
}

/// Whether conversation `id` has a message waiting for a run, or a run of it is live.
async fn is_being_worked_on(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<bool> {
    if conversation::has_untaken(connection, id).await? {
        return Ok(true);
    }
    let live = workflow_run::find_live(
        connection,
        workflow::workflow().name(),
        workflow::SOURCE_KIND,
        &id.to_string(),
    )
    .await?;
    Ok(live.is_some())
}

/// Which conversations went quiet, among the messages no run has taken.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Quiet {
    /// Conversations a run is owed now.
    pub due: Vec<Uuid>,
    /// When the next of the others goes quiet, if one will.
    pub next: Option<DateTime<Utc>>,
}

/// The conversations among `untaken` that a run is owed at `now`: those with a message that wakes,
/// or one a live run's agent was given that still needs a run's segmenting, whose newest message
/// is at least [`QUIET_WINDOW`] old. A conversation whose messages only joined as context waits
/// for one that wakes.
pub fn quiet(untaken: &[ConversationMessage], now: DateTime<Utc>) -> Quiet {
    // Per conversation: whether it owes a run, and its newest message.
    let mut conversations: HashMap<Uuid, (bool, DateTime<Utc>)> = HashMap::new();
    for message in untaken {
        let owes = message.wakes || message.delivered_at.is_some();
        let entry = conversations
            .entry(message.conversation_id)
            .or_insert((false, message.received_at));
        entry.0 |= owes;
        entry.1 = entry.1.max(message.received_at);
    }
    let mut quiet = Quiet::default();
    for (id, (owes, newest)) in conversations {
        if !owes {
            continue;
        }
        let quiet_at = newest + QUIET_WINDOW;
        if quiet_at <= now {
            quiet.due.push(id);
        } else {
            quiet.next = Some(quiet.next.map_or(quiet_at, |next| next.min(quiet_at)));
        }
    }
    quiet.due.sort_unstable();
    quiet
}

/// How a look at one quiet conversation ended.
#[derive(Debug)]
enum Claimed {
    /// A run started, taking its waiting messages.
    Started(Box<Settled>),
    /// A run of it is live, in this state, and carries it.
    Busy(WorkflowRunState),
    /// Nothing is owed after all: someone holds it, a message arrived, or another look took it.
    Nothing,
}

/// Starts request runs for every conversation that goes quiet. Cheap to clone.
#[derive(Clone)]
pub struct Conversations {
    database: Pool,
    events: EventBus,
}

impl std::fmt::Debug for Conversations {
    #[expect(
        clippy::renamed_function_params,
        reason = "project naming rule forbids std's one-letter `f`"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Conversations")
            .finish_non_exhaustive()
    }
}

impl Conversations {
    pub fn new(database: Pool, events: EventBus) -> Self {
        Self { database, events }
    }

    /// Looks for quiet conversations until `shutdown`: when ingest signals a message that wakes,
    /// when the next conversation goes quiet, and every [`LOOK_INTERVAL`] besides.
    pub fn start(&self, shutdown: CancellationToken) -> JoinHandle<()> {
        let conversations = self.clone();
        let mut wake = self
            .events
            .wake_on(|signal| *signal == Signal::Conversations);
        tokio::spawn(async move {
            loop {
                let next_look = conversations.look().await;
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    () = wake.woken() => {}
                    () = tokio::time::sleep(next_look) => {}
                }
            }
            event!(
                name: "conversations.stopped",
                Level::INFO,
                "the conversation loop stopped"
            );
        })
    }

    /// One look: starts a run for each quiet conversation, and answers how long until the next
    /// look is owed.
    async fn look(&self) -> Duration {
        let looked = async {
            let mut connection = self.database.get().await?;
            let now = Utc::now();
            let untaken = conversation::untaken(&mut connection).await?;
            let found = quiet(&untaken, now);
            let mut next_look = found
                .next
                .and_then(|next| (next - now).to_std().ok())
                .unwrap_or(LOOK_INTERVAL)
                .min(LOOK_INTERVAL);
            for id in found.due {
                match claim(&mut connection, id, Utc::now()).await? {
                    Claimed::Started(settled) => {
                        event!(
                            name: "conversations.run.started",
                            Level::INFO,
                            conversation.id = %id,
                            workflow_run.id = %settled.run.id,
                            "a quiet conversation's request run started",
                        );
                        workflows::announce(&self.events, &settled);
                        self.events.signal(&Signal::Workflows);
                    }
                    Claimed::Busy(WorkflowRunState::Queued | WorkflowRunState::Running) => {
                        next_look = next_look.min(BUSY_RECHECK);
                    }
                    // A run waiting on a person is looked at again on the interval.
                    Claimed::Busy(_) | Claimed::Nothing => {}
                }
            }
            anyhow::Ok(next_look)
        };
        match looked.await {
            Ok(next_look) => next_look,
            Err(error) => {
                event!(
                    name: "conversations.look.failure",
                    Level::ERROR,
                    error.message = %format!("{error:#}"),
                    "could not look for quiet conversations; looking again shortly",
                );
                LOOK_INTERVAL
            }
        }
    }
}

/// Starts conversation `id`'s request run when it is still owed one at `now`, taking its waiting
/// messages, in one transaction under the conversation's lock: two looks never both take a
/// message, and a message is never taken by a run that was not stored.
async fn claim(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    now: DateTime<Utc>,
) -> anyhow::Result<Claimed> {
    let claimed = connection
        .transaction(async move |connection| {
            let Some(held) = conversation::lock_unless_held(connection, id).await? else {
                return Ok(Claimed::Nothing);
            };
            let taking = conversation::untaken_of(connection, id, MESSAGES_PER_RUN).await?;
            // Read again under the lock: a message may have arrived since the look.
            if !quiet(&taking, now).due.contains(&id) {
                return Ok(Claimed::Nothing);
            }
            let Some(first) = taking.first() else {
                return Ok(Claimed::Nothing);
            };
            let context =
                conversation::taken_before(connection, id, first.received_at, CONTEXT_MESSAGES)
                    .await?;
            let taken: Vec<Uuid> = taking
                .iter()
                .map(|message| message.timeline_record_id)
                .collect();
            let mut record_ids: Vec<Uuid> = context.into_iter().rev().collect();
            record_ids.extend_from_slice(&taken);

            let started =
                workflow_run::start(connection, workflow::run_for(&held, record_ids), now).await?;
            if !started.created {
                return Ok(Claimed::Busy(started.settled.run.state));
            }
            conversation::take(connection, &taken, started.settled.run.id).await?;
            Ok::<_, diesel::result::Error>(Claimed::Started(Box::new(started.settled)))
        })
        .await?;
    Ok(claimed)
}

#[cfg(test)]
mod tests;
