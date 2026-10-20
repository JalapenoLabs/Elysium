// Copyright © 2026 Jalapeno Labs

//! Elysia's records: her state, the conversation, her turns, her questions, and her work.
//!
//! A message is hers when its creator is [`ELYSIA_USER_ID`]; everything else in the
//! conversation was written by a person. Messages are never edited. Turns move one way, from
//! queued through running to an end, and the database refuses a turn whose timestamps do not
//! match its status. See `docs/elysia.md`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{
    elysia_messages, elysia_questions, elysia_state, elysia_turns, elysia_work,
};
use crate::models::user::ELYSIA_USER_ID;

/// How many messages one page of the conversation holds at most.
pub const MAX_MESSAGE_PAGE: i64 = 100;

/// Where a turn stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ElysiaTurnStatus"]
#[serde(rename_all = "kebab-case")]
pub enum TurnStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

/// Where a question stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ElysiaQuestionStatus"]
#[serde(rename_all = "kebab-case")]
pub enum QuestionStatus {
    Open,
    Answered,
    Dismissed,
}

/// Where a piece of Elysia's work stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ElysiaWorkStatus"]
#[serde(rename_all = "kebab-case")]
pub enum WorkStatus {
    Working,
    Ready,
    Blocked,
    #[db_rename = "gave-up"]
    GaveUp,
    Done,
}

/// The one row of [`elysia_state`].
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = elysia_state, check_for_backend(diesel::pg::Pg))]
pub struct ElysiaState {
    pub paused: bool,
    /// [`crate::elysia::availability::AvailabilityWindow`]s, as stored.
    pub availability: Value,
    pub usage_paused_until: Option<DateTime<Utc>>,
    pub updated_by: Uuid,
    pub updated_at: DateTime<Utc>,
}

/// A partial update of the state a person controls. `None` leaves a column untouched.
#[derive(Debug, Default, AsChangeset)]
#[diesel(table_name = elysia_state)]
pub struct ElysiaStateChanges {
    pub paused: Option<bool>,
    pub availability: Option<Value>,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = elysia_messages, check_for_backend(diesel::pg::Pg))]
pub struct Message {
    pub id: Uuid,
    pub body: String,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

impl Message {
    /// Whether Elysia wrote it, rather than a person.
    pub fn is_from_elysia(&self) -> bool {
        self.created_by == ELYSIA_USER_ID
    }
}

#[derive(Insertable)]
#[diesel(table_name = elysia_messages)]
struct MessageRow<'a> {
    id: Uuid,
    body: &'a str,
    created_by: Uuid,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = elysia_turns, check_for_backend(diesel::pg::Pg))]
pub struct Turn {
    pub id: Uuid,
    pub trigger_kind: String,
    pub trigger_detail: Value,
    pub status: TurnStatus,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

/// A turn to queue.
#[derive(Debug, Insertable)]
#[diesel(table_name = elysia_turns)]
pub struct NewTurn {
    pub id: Uuid,
    pub trigger_kind: String,
    pub trigger_detail: Value,
    pub created_by: Uuid,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = elysia_questions, check_for_backend(diesel::pg::Pg))]
pub struct Question {
    pub id: Uuid,
    pub prompt: String,
    /// A JSON array of suggested answers.
    pub options: Value,
    pub status: QuestionStatus,
    pub answer: Option<String>,
    pub action_item_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub answered_by: Option<Uuid>,
    pub answered_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = elysia_work, check_for_backend(diesel::pg::Pg))]
pub struct Work {
    pub id: Uuid,
    pub title: String,
    pub detail: String,
    pub status: WorkStatus,
    pub ready_at: Option<DateTime<Utc>>,
    pub action_item_id: Option<Uuid>,
    pub revisits: i32,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The state row.
///
/// # Errors
/// Propagates any database error.
pub async fn state(connection: &mut AsyncPgConnection) -> QueryResult<ElysiaState> {
    elysia_state::table
        .select(ElysiaState::as_select())
        .first(connection)
        .await
}

/// Applies a person's `changes` to the state.
///
/// # Errors
/// Propagates any database error.
pub async fn update_state(
    connection: &mut AsyncPgConnection,
    changes: ElysiaStateChanges,
    actor_id: Uuid,
) -> QueryResult<ElysiaState> {
    diesel::update(elysia_state::table)
        .set((changes, elysia_state::updated_by.eq(actor_id)))
        .returning(ElysiaState::as_returning())
        .get_result(connection)
        .await
}

/// Records that nothing may start before `until`, or lifts the hold with `None`.
///
/// # Errors
/// Propagates any database error.
pub async fn set_usage_paused_until(
    connection: &mut AsyncPgConnection,
    until: Option<DateTime<Utc>>,
) -> QueryResult<ElysiaState> {
    diesel::update(elysia_state::table)
        .set(elysia_state::usage_paused_until.eq(until))
        .returning(ElysiaState::as_returning())
        .get_result(connection)
        .await
}

/// Adds a message to the conversation, written by `created_by`.
///
/// # Errors
/// Propagates any database error, including a body outside 1 to 20,000 characters.
pub async fn insert_message(
    connection: &mut AsyncPgConnection,
    body: &str,
    created_by: Uuid,
) -> QueryResult<Message> {
    diesel::insert_into(elysia_messages::table)
        .values(MessageRow {
            id: Uuid::now_v7(),
            body,
            created_by,
        })
        .returning(Message::as_returning())
        .get_result(connection)
        .await
}

/// One page of the conversation, newest first: the `limit` messages sent before the message
/// `before`, or the latest ones without it.
///
/// # Errors
/// Propagates any database error.
pub async fn list_messages(
    connection: &mut AsyncPgConnection,
    before: Option<Uuid>,
    limit: i64,
) -> QueryResult<Vec<Message>> {
    let mut query = elysia_messages::table
        .select(Message::as_select())
        .order(elysia_messages::id.desc())
        .limit(limit.clamp(1, MAX_MESSAGE_PAGE))
        .into_boxed();
    if let Some(before) = before {
        query = query.filter(elysia_messages::id.lt(before));
    }
    query.load(connection).await
}

/// Queues a turn.
///
/// # Errors
/// Propagates any database error, including a malformed trigger kind.
pub async fn insert_turn(connection: &mut AsyncPgConnection, turn: NewTurn) -> QueryResult<Turn> {
    diesel::insert_into(elysia_turns::table)
        .values(turn)
        .returning(Turn::as_returning())
        .get_result(connection)
        .await
}

/// The latest turns, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_turns(connection: &mut AsyncPgConnection, limit: i64) -> QueryResult<Vec<Turn>> {
    elysia_turns::table
        .select(Turn::as_select())
        .order(elysia_turns::id.desc())
        .limit(limit.clamp(1, MAX_MESSAGE_PAGE))
        .load(connection)
        .await
}

/// Every turn that is queued or running, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn active_turns(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Turn>> {
    elysia_turns::table
        .filter(elysia_turns::status.eq_any([TurnStatus::Queued, TurnStatus::Running]))
        .select(Turn::as_select())
        .order(elysia_turns::created_at.asc())
        .load(connection)
        .await
}

/// The most recent turn that ended, succeeded or failed.
///
/// # Errors
/// Propagates any database error.
pub async fn last_finished_turn(connection: &mut AsyncPgConnection) -> QueryResult<Option<Turn>> {
    elysia_turns::table
        .filter(elysia_turns::status.eq_any([TurnStatus::Succeeded, TurnStatus::Failed]))
        .select(Turn::as_select())
        .order(elysia_turns::finished_at.desc())
        .first(connection)
        .await
        .optional()
}

/// Withdraws the queued turn `id`. `None` when it is no longer queued.
///
/// # Errors
/// Propagates any database error.
pub async fn cancel_turn(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Option<Turn>> {
    diesel::update(
        elysia_turns::table
            .find(id)
            .filter(elysia_turns::status.eq(TurnStatus::Queued)),
    )
    .set(elysia_turns::status.eq(TurnStatus::Cancelled))
    .returning(Turn::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// The oldest queued turn of `trigger_kind`, if any.
///
/// # Errors
/// Propagates any database error.
pub async fn queued_turn_of_kind(
    connection: &mut AsyncPgConnection,
    trigger_kind: &str,
) -> QueryResult<Option<Turn>> {
    elysia_turns::table
        .filter(elysia_turns::status.eq(TurnStatus::Queued))
        .filter(elysia_turns::trigger_kind.eq(trigger_kind))
        .select(Turn::as_select())
        .order(elysia_turns::created_at.asc())
        .first(connection)
        .await
        .optional()
}

/// Replaces what the queued turn `id` carries. `None` when it is no longer queued.
///
/// # Errors
/// Propagates any database error.
pub async fn set_trigger_detail(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    detail: Value,
) -> QueryResult<Option<Turn>> {
    diesel::update(
        elysia_turns::table
            .find(id)
            .filter(elysia_turns::status.eq(TurnStatus::Queued)),
    )
    .set(elysia_turns::trigger_detail.eq(detail))
    .returning(Turn::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Every open question, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn open_questions(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Question>> {
    elysia_questions::table
        .filter(elysia_questions::status.eq(QuestionStatus::Open))
        .select(Question::as_select())
        .order(elysia_questions::created_at.asc())
        .load(connection)
        .await
}

/// Closes the open question `id`: answered with `answer`, or dismissed with `None`. `None`
/// when it is missing or no longer open.
///
/// # Errors
/// Propagates any database error.
pub async fn close_question(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    answer: Option<&str>,
    actor_id: Uuid,
) -> QueryResult<Option<Question>> {
    let status = if answer.is_some() {
        QuestionStatus::Answered
    } else {
        QuestionStatus::Dismissed
    };
    diesel::update(
        elysia_questions::table
            .find(id)
            .filter(elysia_questions::status.eq(QuestionStatus::Open)),
    )
    .set((
        elysia_questions::status.eq(status),
        elysia_questions::answer.eq(answer),
        elysia_questions::answered_by.eq(actor_id),
        elysia_questions::answered_at.eq(diesel::dsl::now),
    ))
    .returning(Question::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Everything not done, most recently changed first.
///
/// # Errors
/// Propagates any database error.
pub async fn unfinished_work(connection: &mut AsyncPgConnection) -> QueryResult<Vec<Work>> {
    elysia_work::table
        .filter(elysia_work::status.ne(WorkStatus::Done))
        .select(Work::as_select())
        .order(elysia_work::updated_at.desc())
        .load(connection)
        .await
}
