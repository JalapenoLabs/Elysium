// Copyright © 2026 Jalapeno Labs

//! Every project's Project Manager: one Claude Code session per project that reads what lands on
//! its project, decides what work comes next, and submits that work as batches for the engine's
//! worker pool. See `docs/engine.md`, Project Managers.
//!
//! It is woken by records attributed to its project (a meeting's or a conversation's topics, or a
//! record a person put there), by the results of its batches, by a person, and by Elysia. Each of
//! those writes a wake in the transaction that caused it (`crate::models::project_manager`), and
//! the engine's [`dispatcher`] turns a project's wakes into one turn once they go quiet
//! ([`queue`]). Every turn resumes the project's one session (`claude -p --resume`), relying on
//! Claude Code's own compaction as it grows, on the model its kind resolves to for the project.
//!
//! What it decides lands on its project's timeline ([`timeline`]): each turn that did its work,
//! each batch it submits ([`batches`]), and each change it makes. Its changes to action items and
//! their trackers (an item, a comment, a status change, an item's own issue) are changesets
//! (`crate::tools::work::manager_changes`), which wait for a person's approval unless its project
//! lets it write directly, and its issues file in the project's tracker ([`trackers`]).
//!
//! - [`queue`]: when each project's next turn is due.
//! - [`dispatcher`]: starts the turns that are due and records how they end.
//! - [`prompt`]: its standing instructions and each turn's prompt.
//! - [`runner`]: the seam to the host runner, and each turn's log.
//! - [`batches`]: submitting and withdrawing work batches.
//! - [`timeline`]: what it puts on its project's timeline.
//! - [`trackers`]: where its project's issues file, and through which credential.
//! - [`status`]: what its project's page shows at a glance.

pub mod batches;
pub mod dispatcher;
pub mod promises;
pub mod prompt;
pub mod queue;
pub mod runner;
pub mod status;
#[cfg(test)]
mod tests;
pub mod timeline;
pub mod trackers;

use std::collections::HashMap;

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use self::dispatcher::DispatchContext;
use self::runner::ManagerRunner;
use self::status::{Status, StatusInputs};
use crate::database::Pool;
use crate::models::action_item_link::LinkProvider;
use crate::models::draft;
use crate::models::project_manager::{self, ProjectManager, Turn, TurnStatus, Wake};
use crate::models::promise::{self, Promise};
use crate::models::timeline_record::{self, TimelineRecord};
use crate::models::work_batch::{self, WorkBatch};
use crate::models::{github_credential, jira_credential, project, token_account};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::tools::grants::Grants;

/// How many of a project's latest turns its page shows.
pub const TURNS_SHOWN: i64 = 30;

/// How many of a project's latest batches its page shows.
pub const BATCHES_SHOWN: i64 = 100;

/// How many of a project's latest promises its page shows.
pub const PROMISES_SHOWN: i64 = 100;

/// Starts the dispatcher, which runs every Project Manager's turns until `shutdown`. Whatever
/// wakes one, in any process, sends [`Signal::ProjectManagers`], so it looks at once.
pub fn start(
    database: Pool,
    events: EventBus,
    runner: ManagerRunner,
    grants: Grants,
    shutdown: CancellationToken,
) -> JoinHandle<()> {
    tokio::spawn(dispatcher::run(
        DispatchContext {
            database,
            events,
            runner,
            grants,
        },
        shutdown,
    ))
}

/// Wakes `project_id`'s Project Manager with `body`, written by `author`, a person or Elysia,
/// and announces it. Its turn starts as soon as it is free.
///
/// # Errors
/// Propagates any database error, including a project that does not exist.
pub async fn message(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    project_id: Uuid,
    body: &str,
    author: Uuid,
) -> diesel::QueryResult<Wake> {
    let wake = project_manager::wake_with_message(connection, project_id, body, author).await?;
    events.publish(&ServerEvent::ProjectManagerChanged { project_id });
    events.signal(&Signal::ProjectManagers);
    Ok(wake)
}

/// Everything a project's page shows of its Project Manager.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub project_id: Uuid,
    /// `None` until something has woken it.
    pub state: Option<ProjectManager>,
    pub status: Status,
    /// How many wakes wait for its next turn.
    pub waiting: i64,
    /// Its latest turns, newest first, each with what woke it.
    pub turns: Vec<(Turn, Vec<Wake>)>,
    /// Its latest batches, newest first.
    pub batches: Vec<WorkBatch>,
    /// Its latest promises, newest first, each with the message it was made in while that is
    /// still there, and its follow-up's draft once drafted.
    pub promises: Vec<(Promise, Option<TimelineRecord>, Option<Uuid>)>,
    /// How its changes reach the work.
    pub writes: WritesSnapshot,
}

/// How a Project Manager's changes reach the work, as its tab shows it.
#[derive(Debug, Clone)]
pub struct WritesSnapshot {
    /// Whether its changes apply as it makes them, rather than waiting for a person.
    pub direct: bool,
    /// Where its issues file, or why there is nowhere.
    pub tracker: Result<TrackerSnapshot, &'static str>,
}

/// A project's tracker, with the name of the credential that files there.
#[derive(Debug, Clone)]
pub struct TrackerSnapshot {
    pub tracker: trackers::Tracker,
    /// The Jira credential's, GitHub token's, or YouTrack account's name.
    pub credential_name: String,
}

/// Reads `project_id`'s Project Manager and derives its status.
///
/// # Errors
/// Propagates any database error.
pub async fn snapshot(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> diesel::QueryResult<Snapshot> {
    let state = project_manager::find(connection, project_id).await?;
    let waiting = project_manager::waiting_count(connection, project_id).await?;
    let turns = project_manager::list_turns(connection, project_id, TURNS_SHOWN).await?;
    let turn_ids: Vec<Uuid> = turns.iter().map(|turn| turn.id).collect();
    let batches = work_batch::list_for_project(connection, project_id, &[], BATCHES_SHOWN).await?;
    let made = promise::list_for_project(connection, project_id, &[], PROMISES_SHOWN).await?;
    let record_ids: Vec<Uuid> = made.iter().map(|made| made.timeline_record_id).collect();
    let mut records: HashMap<Uuid, TimelineRecord> =
        timeline_record::find_many(connection, &record_ids)
            .await?
            .into_iter()
            .map(|record| (record.id, record))
            .collect();
    let promise_ids: Vec<Uuid> = made.iter().map(|made| made.id).collect();
    let drafts: HashMap<Uuid, Uuid> = draft::ids_for_promises(connection, &promise_ids)
        .await?
        .into_iter()
        .collect();
    let promises = made
        .into_iter()
        .map(|made| {
            let record = records.remove(&made.timeline_record_id);
            let draft_id = drafts.get(&made.id).copied();
            (made, record, draft_id)
        })
        .collect();

    let status = status::derive(&StatusInputs {
        is_running: turns
            .first()
            .is_some_and(|turn| turn.status == TurnStatus::Running),
        waiting,
        usage_paused_until: state
            .as_ref()
            .and_then(|manager| manager.usage_paused_until),
        last_turn_failed: turns
            .iter()
            .find(|turn| turn.status != TurnStatus::Running)
            .is_some_and(|turn| turn.status == TurnStatus::Failed),
        now: Utc::now(),
    });

    let direct = project::find(connection, project_id)
        .await?
        .manager_writes_directly;
    let tracker = match trackers::resolve(connection, project_id).await? {
        Ok(tracker) => Ok(TrackerSnapshot {
            credential_name: credential_name(connection, &tracker).await?,
            tracker,
        }),
        Err(reason) => Err(reason),
    };

    let mut by_turn: HashMap<Uuid, Vec<Wake>> = HashMap::new();
    for wake in project_manager::wakes_of(connection, &turn_ids).await? {
        if let Some(turn_id) = wake.turn_id {
            by_turn.entry(turn_id).or_default().push(wake);
        }
    }
    let turns = turns
        .into_iter()
        .map(|turn| {
            let taken = by_turn.remove(&turn.id).unwrap_or_default();
            (turn, taken)
        })
        .collect();
    Ok(Snapshot {
        project_id,
        state,
        status,
        waiting,
        turns,
        batches,
        promises,
        writes: WritesSnapshot { direct, tracker },
    })
}

/// The name of the credential `tracker` files through, as its settings page names it.
async fn credential_name(
    connection: &mut AsyncPgConnection,
    tracker: &trackers::Tracker,
) -> diesel::QueryResult<String> {
    let id = tracker.credential_id;
    Ok(match tracker.place.provider {
        LinkProvider::Jira => jira_credential::find(connection, id).await?.name,
        LinkProvider::Github => github_credential::find(connection, id).await?.name,
        LinkProvider::Youtrack => token_account::find(connection, id).await?.name,
    })
}
