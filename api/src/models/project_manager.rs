// Copyright © 2026 Jalapeno Labs

//! Every project's Project Manager: its state, its turns, and what wakes it. See
//! `docs/engine.md`, Project Managers, and `crate::project_managers` for what runs them.
//!
//! # Wakes are the queue
//!
//! Whatever should wake a Project Manager writes a wake (`project_manager_wakes`) in the same
//! transaction as what caused it: a record attributed to its project, a batch of its finishing,
//! a person's or Elysia's message, a promise found on its project or its work finishing, someone
//! else publishing its project's brief, or an action item a person assigned it. So nothing that
//! should wake it is lost to a crash or a missed signal. A wake waits until a turn takes it.
//!
//! A turn is created as it starts ([`start_turn`]), taking every wake waiting for its project in
//! the same transaction, so a burst of wakes is one turn, and wakes arriving while it runs wait
//! for the next. A project has at most one running turn: the database refuses a second
//! (`project_manager_turns_one_running`). A turn that never finished its work, because the
//! engine stopped or every credential was spent, gives its wakes back ([`finish_turn`],
//! [`fail_interrupted_turns`]), so the next turn takes them again.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::database::schema::{project_manager_turns, project_manager_wakes, project_managers};
use crate::models::user::PROJECT_MANAGER_USER_ID;

/// How many turns one page of a Project Manager's turns holds at most.
pub const MAX_TURN_PAGE: i64 = 100;

/// Where a turn stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ProjectManagerTurnStatus"]
#[serde(rename_all = "kebab-case")]
pub enum TurnStatus {
    Running,
    Succeeded,
    Failed,
}

/// What woke a Project Manager.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ProjectManagerWakeKind"]
#[serde(rename_all = "kebab-case")]
pub enum WakeKind {
    /// A record was attributed to its project.
    Record,
    /// A batch it submitted finished.
    Batch,
    /// A person or Elysia wrote to it.
    Message,
    /// A promise was found on its project, or the work it waits on finished
    /// (`crate::models::promise`).
    Promise,
    /// Someone else published its project's brief (`crate::models::project_brief`).
    Brief,
    /// A person assigned it an action item its project leads, to plan
    /// (`crate::models::action_item_assignment`).
    Assignment,
}

/// A project's Project Manager, once something has woken it.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = project_managers, check_for_backend(diesel::pg::Pg))]
pub struct ProjectManager {
    /// The Claude Code session the next turn resumes.
    pub runner_session_id: Option<String>,
    /// No turn starts before this, after every credential reported a usage limit.
    pub usage_paused_until: Option<DateTime<Utc>>,
}

/// One turn, without its log, which is read alone ([`find_turn`]).
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = project_manager_turns, check_for_backend(diesel::pg::Pg))]
pub struct Turn {
    pub id: Uuid,
    pub project_id: Uuid,
    pub status: TurnStatus,
    pub summary: Option<String>,
    pub reply: Option<String>,
    pub error: Option<String>,
    pub timeline_record_id: Option<Uuid>,
    pub created_by: Uuid,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// One wake.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = project_manager_wakes, check_for_backend(diesel::pg::Pg))]
pub struct Wake {
    pub id: Uuid,
    pub project_id: Uuid,
    pub kind: WakeKind,
    pub timeline_record_id: Option<Uuid>,
    pub work_batch_id: Option<Uuid>,
    pub body: Option<String>,
    /// The turn that took it; `None` while it waits.
    pub turn_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub promise_id: Option<Uuid>,
    /// The version of the brief a publish made, for a brief's wake.
    pub page_version: Option<i32>,
    /// The action item an assignment is for.
    pub action_item_id: Option<Uuid>,
}

/// What one waiting wake says about when its project's next turn is due: its project, its
/// kind, and when it arrived. Enough for `crate::project_managers::queue` to decide, without the
/// records or messages it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Queryable)]
pub struct WaitingWake {
    pub project_id: Uuid,
    pub kind: WakeKind,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = project_manager_wakes)]
struct NewWake<'a> {
    id: Uuid,
    project_id: Uuid,
    kind: WakeKind,
    timeline_record_id: Option<Uuid>,
    work_batch_id: Option<Uuid>,
    body: Option<&'a str>,
    promise_id: Option<Uuid>,
    page_version: Option<i32>,
    action_item_id: Option<Uuid>,
    created_by: Uuid,
}

impl NewWake<'_> {
    const fn new(project_id: Uuid, kind: WakeKind, created_by: Uuid) -> Self {
        Self {
            id: Uuid::nil(),
            project_id,
            kind,
            timeline_record_id: None,
            work_batch_id: None,
            body: None,
            promise_id: None,
            page_version: None,
            action_item_id: None,
            created_by,
        }
    }

    async fn insert(mut self, connection: &mut AsyncPgConnection) -> QueryResult<Option<Wake>> {
        self.id = Uuid::now_v7();
        // A record, a promise, or an assigned item already waiting for this project is not woken
        // for twice (`project_manager_wakes_record_waits_once`, `..._promise_waits_once`,
        // `..._assignment_waits_once`); every other wake is new.
        diesel::insert_into(project_manager_wakes::table)
            .values(self)
            .on_conflict_do_nothing()
            .returning(Wake::as_returning())
            .get_result(connection)
            .await
            .optional()
    }
}

/// Wakes `project_id`'s Project Manager for record `record_id`, which `by` just attributed to
/// the project. A record already waiting is not woken for twice. Call it in the transaction
/// that attributes the record.
///
/// # Errors
/// Propagates any database error.
pub async fn wake_for_record(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    record_id: Uuid,
    by: Uuid,
) -> QueryResult<()> {
    let mut wake = NewWake::new(project_id, WakeKind::Record, by);
    wake.timeline_record_id = Some(record_id);
    wake.insert(connection).await.map(drop)
}

/// Wakes `project_id`'s Project Manager for its batch `batch_id`, which just finished, as
/// `by`. Call it in the transaction that records the batch's end.
///
/// # Errors
/// Propagates any database error.
pub async fn wake_for_batch(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    batch_id: Uuid,
    by: Uuid,
) -> QueryResult<()> {
    let mut wake = NewWake::new(project_id, WakeKind::Batch, by);
    wake.work_batch_id = Some(batch_id);
    wake.insert(connection).await.map(drop)
}

/// Wakes `project_id`'s Project Manager for its promise `promise_id`, found just now or whose work
/// just finished, as `by`. A promise already waiting is not woken for twice. Call it in the
/// transaction that records the promise or its work finishing.
///
/// # Errors
/// Propagates any database error.
pub async fn wake_for_promise(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    promise_id: Uuid,
    by: Uuid,
) -> QueryResult<()> {
    let mut wake = NewWake::new(project_id, WakeKind::Promise, by);
    wake.promise_id = Some(promise_id);
    wake.insert(connection).await.map(drop)
}

/// Wakes `project_id`'s Project Manager for version `version` of its project's brief, which `by`
/// just published. Each publish is a wake of its own, so the turn names everyone who edited the
/// brief since the last. Call it in the transaction that publishes the version.
///
/// # Errors
/// Propagates any database error.
pub async fn wake_for_brief(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    version: i32,
    by: Uuid,
) -> QueryResult<()> {
    let mut wake = NewWake::new(project_id, WakeKind::Brief, by);
    wake.page_version = Some(version);
    wake.insert(connection).await.map(drop)
}

/// Wakes `project_id`'s Project Manager to plan action item `item_id`, which person `by` just
/// assigned it. An item already waiting is not woken for twice. Call it in the transaction that
/// assigns the item.
///
/// # Errors
/// Propagates any database error.
pub async fn wake_for_assignment(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    item_id: Uuid,
    by: Uuid,
) -> QueryResult<()> {
    let mut wake = NewWake::new(project_id, WakeKind::Assignment, by);
    wake.action_item_id = Some(item_id);
    wake.insert(connection).await.map(drop)
}

/// Wakes `project_id`'s Project Manager with `body`, written by `by`: a person, or Elysia.
///
/// # Errors
/// Propagates any database error, including a body outside 1 to 20,000 characters and a
/// project that does not exist.
pub async fn wake_with_message(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    body: &str,
    by: Uuid,
) -> QueryResult<Wake> {
    let mut wake = NewWake::new(project_id, WakeKind::Message, by);
    wake.body = Some(body);
    wake.insert(connection)
        .await?
        .ok_or(diesel::result::Error::NotFound)
}

/// Whether an assignment of item `item_id` waits for a turn no turn has taken yet.
///
/// # Errors
/// Propagates any database error.
pub async fn waiting_assignment(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        project_manager_wakes::table
            .filter(project_manager_wakes::kind.eq(WakeKind::Assignment))
            .filter(project_manager_wakes::action_item_id.eq(item_id))
            .filter(project_manager_wakes::turn_id.is_null()),
    ))
    .get_result(connection)
    .await
}

/// Every waiting wake, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn waiting(connection: &mut AsyncPgConnection) -> QueryResult<Vec<WaitingWake>> {
    project_manager_wakes::table
        .filter(project_manager_wakes::turn_id.is_null())
        .select((
            project_manager_wakes::project_id,
            project_manager_wakes::kind,
            project_manager_wakes::created_at,
        ))
        .order(project_manager_wakes::created_at.asc())
        .load(connection)
        .await
}

/// How many wakes wait for `project_id`'s next turn.
///
/// # Errors
/// Propagates any database error.
pub async fn waiting_count(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<i64> {
    project_manager_wakes::table
        .filter(project_manager_wakes::project_id.eq(project_id))
        .filter(project_manager_wakes::turn_id.is_null())
        .count()
        .get_result(connection)
        .await
}

/// `project_id`'s Project Manager, if anything has woken it yet.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Option<ProjectManager>> {
    project_managers::table
        .find(project_id)
        .select(ProjectManager::as_select())
        .first(connection)
        .await
        .optional()
}

/// Every Project Manager a usage limit holds at `now`, with when it lifts.
///
/// # Errors
/// Propagates any database error.
pub async fn held(
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> QueryResult<Vec<(Uuid, DateTime<Utc>)>> {
    let rows: Vec<(Uuid, Option<DateTime<Utc>>)> = project_managers::table
        .filter(project_managers::usage_paused_until.gt(now))
        .select((
            project_managers::project_id,
            project_managers::usage_paused_until,
        ))
        .load(connection)
        .await?;
    Ok(rows
        .into_iter()
        .filter_map(|(project_id, until)| Some((project_id, until?)))
        .collect())
}

/// The projects whose Project Manager has a turn running.
///
/// # Errors
/// Propagates any database error.
pub async fn running_projects(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Uuid>> {
    project_manager_turns::table
        .filter(project_manager_turns::status.eq(TurnStatus::Running))
        .select(project_manager_turns::project_id)
        .load(connection)
        .await
}

/// Starts a turn for `project_id`, taking up to `max_wakes` of its waiting wakes, oldest first,
/// and every waiting message whatever their number, since a person's words are never left for a
/// later turn. Creates the Project Manager's row the first time.
///
/// `None` when it may not start: a turn of the project is already running, or nothing waits.
///
/// # Errors
/// Propagates any database error.
pub async fn start_turn(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    max_wakes: i64,
) -> QueryResult<Option<(Turn, Vec<Wake>)>> {
    connection
        .transaction(async |connection| {
            diesel::insert_into(project_managers::table)
                .values((
                    project_managers::project_id.eq(project_id),
                    project_managers::created_by.eq(PROJECT_MANAGER_USER_ID),
                ))
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
            // The one-running index refuses a second turn; nothing is inserted then.
            let Some(turn) = diesel::insert_into(project_manager_turns::table)
                .values((
                    project_manager_turns::id.eq(Uuid::now_v7()),
                    project_manager_turns::project_id.eq(project_id),
                    project_manager_turns::created_by.eq(PROJECT_MANAGER_USER_ID),
                ))
                .on_conflict_do_nothing()
                .returning(Turn::as_returning())
                .get_result(connection)
                .await
                .optional()?
            else {
                return Ok(None);
            };

            // A second start for the project waits on the insert above until this one commits,
            // then inserts nothing, so reading the wakes before taking them is safe.
            let waiting = project_manager_wakes::table
                .filter(project_manager_wakes::project_id.eq(project_id))
                .filter(project_manager_wakes::turn_id.is_null());
            let mut ids: Vec<Uuid> = waiting
                .select(project_manager_wakes::id)
                .order(project_manager_wakes::created_at.asc())
                .limit(max_wakes)
                .load(connection)
                .await?;
            let messages: Vec<Uuid> = waiting
                .filter(project_manager_wakes::kind.eq(WakeKind::Message))
                .select(project_manager_wakes::id)
                .load(connection)
                .await?;
            ids.extend(messages);
            let mut taken: Vec<Wake> = diesel::update(
                project_manager_wakes::table.filter(project_manager_wakes::id.eq_any(ids)),
            )
            .set(project_manager_wakes::turn_id.eq(turn.id))
            .returning(Wake::as_returning())
            .get_results(connection)
            .await?;
            if taken.is_empty() {
                // Another turn took them first; this one has nothing to do.
                return Err(diesel::result::Error::RollbackTransaction);
            }
            taken.sort_by_key(|wake| wake.id);
            Ok(Some((turn, taken)))
        })
        .await
        .or_else(|error| match error {
            diesel::result::Error::RollbackTransaction => Ok(None),
            error => Err(error),
        })
}

/// How a turn ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Finished<'a> {
    /// It did its work: its summary, its reply, and the record it put on the timeline.
    Succeeded {
        summary: Option<&'a str>,
        reply: Option<&'a str>,
        timeline_record_id: Option<Uuid>,
    },
    /// It failed. `retry` gives its wakes back for the next turn, as a usage limit does; a
    /// turn that failed of itself keeps them, since another try would likely fail the same way.
    Failed { error: &'a str, retry: bool },
}

/// Ends the running turn `turn_id` as `finished`, with its `log`.
///
/// # Errors
/// Propagates any database error, including a turn that is not running.
pub async fn finish_turn(
    connection: &mut AsyncPgConnection,
    turn_id: Uuid,
    finished: Finished<'_>,
    log: &str,
) -> QueryResult<Turn> {
    let (status, summary, reply, error, timeline_record_id, retry) = match finished {
        Finished::Succeeded {
            summary,
            reply,
            timeline_record_id,
        } => (
            TurnStatus::Succeeded,
            summary,
            reply,
            None,
            timeline_record_id,
            false,
        ),
        Finished::Failed { error, retry } => {
            (TurnStatus::Failed, None, None, Some(error), None, retry)
        }
    };
    connection
        .transaction(async |connection| {
            let turn = diesel::update(
                project_manager_turns::table
                    .find(turn_id)
                    .filter(project_manager_turns::status.eq(TurnStatus::Running)),
            )
            .set((
                project_manager_turns::status.eq(status),
                project_manager_turns::summary.eq(summary),
                project_manager_turns::reply.eq(reply),
                project_manager_turns::error.eq(error),
                project_manager_turns::log.eq(log),
                project_manager_turns::timeline_record_id.eq(timeline_record_id),
                project_manager_turns::finished_at.eq(diesel::dsl::now),
            ))
            .returning(Turn::as_returning())
            .get_result(connection)
            .await?;
            if retry {
                release_wakes(connection, &[turn_id]).await?;
            }
            Ok(turn)
        })
        .await
}

/// Fails every turn left running, which only an engine that stopped mid-turn leaves, with
/// `error`, and gives their wakes back so the next turns take them again.
///
/// # Errors
/// Propagates any database error.
pub async fn fail_interrupted_turns(
    connection: &mut AsyncPgConnection,
    error: &str,
) -> QueryResult<Vec<Turn>> {
    connection
        .transaction(async |connection| {
            let failed: Vec<Turn> = diesel::update(
                project_manager_turns::table
                    .filter(project_manager_turns::status.eq(TurnStatus::Running)),
            )
            .set((
                project_manager_turns::status.eq(TurnStatus::Failed),
                project_manager_turns::error.eq(error),
                project_manager_turns::finished_at.eq(diesel::dsl::now),
            ))
            .returning(Turn::as_returning())
            .get_results(connection)
            .await?;
            let ids: Vec<Uuid> = failed.iter().map(|turn| turn.id).collect();
            release_wakes(connection, &ids).await?;
            Ok(failed)
        })
        .await
}

/// Gives the wakes the turns `turn_ids` took back, to wait for the next turn. A record or a
/// promise woken for again meanwhile already waits, so its taken copy is dropped rather than
/// waiting twice.
async fn release_wakes(connection: &mut AsyncPgConnection, turn_ids: &[Uuid]) -> QueryResult<()> {
    diesel::sql_query(
        "DELETE FROM project_manager_wakes AS taken \
         WHERE taken.turn_id = ANY($1) \
           AND EXISTS ( \
             SELECT 1 FROM project_manager_wakes AS waiting \
             WHERE waiting.turn_id IS NULL \
               AND waiting.project_id = taken.project_id \
               AND (waiting.timeline_record_id = taken.timeline_record_id \
                    OR waiting.promise_id = taken.promise_id) \
           )",
    )
    .bind::<diesel::sql_types::Array<diesel::sql_types::Uuid>, _>(turn_ids)
    .execute(connection)
    .await?;
    diesel::update(
        project_manager_wakes::table.filter(project_manager_wakes::turn_id.eq_any(turn_ids)),
    )
    .set(project_manager_wakes::turn_id.eq(None::<Uuid>))
    .execute(connection)
    .await
    .map(drop)
}

/// Records the session `project_id`'s next turn resumes.
///
/// # Errors
/// Propagates any database error.
pub async fn set_runner_session_id(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    session_id: &str,
) -> QueryResult<()> {
    diesel::update(project_managers::table.find(project_id))
        .set(project_managers::runner_session_id.eq(session_id))
        .execute(connection)
        .await
        .map(drop)
}

/// Holds `project_id`'s Project Manager until `until`, after every credential reported a usage
/// limit.
///
/// # Errors
/// Propagates any database error.
pub async fn hold_until(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    until: DateTime<Utc>,
) -> QueryResult<()> {
    diesel::update(project_managers::table.find(project_id))
        .set(project_managers::usage_paused_until.eq(until))
        .execute(connection)
        .await
        .map(drop)
}

/// `project_id`'s latest turns, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_turns(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    limit: i64,
) -> QueryResult<Vec<Turn>> {
    project_manager_turns::table
        .filter(project_manager_turns::project_id.eq(project_id))
        .select(Turn::as_select())
        .order(project_manager_turns::id.desc())
        .limit(limit.clamp(1, MAX_TURN_PAGE))
        .load(connection)
        .await
}

/// Turn `turn_id` of `project_id`, with its log.
///
/// # Errors
/// `NotFound` when the project has no such turn; any other database error.
pub async fn find_turn(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    turn_id: Uuid,
) -> QueryResult<(Turn, String)> {
    project_manager_turns::table
        .find(turn_id)
        .filter(project_manager_turns::project_id.eq(project_id))
        .select((Turn::as_select(), project_manager_turns::log))
        .first(connection)
        .await
}

/// Every wake the turns `turn_ids` took, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn wakes_of(
    connection: &mut AsyncPgConnection,
    turn_ids: &[Uuid],
) -> QueryResult<Vec<Wake>> {
    project_manager_wakes::table
        .filter(project_manager_wakes::turn_id.eq_any(turn_ids))
        .select(Wake::as_select())
        .order(project_manager_wakes::id.asc())
        .load(connection)
        .await
}
