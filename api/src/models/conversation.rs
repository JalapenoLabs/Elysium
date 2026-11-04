// Copyright © 2026 Jalapeno Labs

//! Conversations and the messages they gather: the rows the engine's conversation work is built
//! on. See `crate::conversations` and `docs/engine.md`, Conversations.
//!
//! A message moves through two marks, each set once:
//!
//! - **taken** (`run_id`): a request run took it, and its segmenting reads it;
//! - **delivered** (`delivered_at`): a handling agent was given it.
//!
//! A message that arrives while a run is live is delivered to that run's agent first and taken by
//! the next run, so every message is both read by segmenting and seen by an agent, once each.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::{conversation_messages, conversations};

/// The longest key and title the columns' checks allow.
pub const KEY_MAX_CHARACTERS: usize = 1024;
pub const TITLE_MAX_CHARACTERS: usize = 2048;

#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = conversations, check_for_backend(diesel::pg::Pg))]
pub struct Conversation {
    pub id: Uuid,
    pub connector_id: Uuid,
    /// The conversation in its kind's own terms (`crate::connectors::Conversation`).
    pub key: String,
    /// What it is called where it is had, as its newest message named it.
    pub title: String,
    /// The Claude session its handling agent's turns resume.
    pub harness_session_id: Option<String>,
    /// When a wait its handling agent asked for ends; the agent is owed a turn after it.
    pub waiting_until: Option<DateTime<Utc>>,
    /// The run whose agent asked to wait, which may not wait again.
    pub waited_in_run_id: Option<Uuid>,
    /// Whose conversation it is: the connector's person.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = conversation_messages, check_for_backend(diesel::pg::Pg))]
pub struct ConversationMessage {
    pub timeline_record_id: Uuid,
    pub conversation_id: Uuid,
    /// Whether it asks something of the person, which is what starts work.
    pub wakes: bool,
    /// When Elysium read it.
    pub received_at: DateTime<Utc>,
    /// The request run that took it.
    pub run_id: Option<Uuid>,
    /// When a handling agent was given it.
    pub delivered_at: Option<DateTime<Utc>>,
}

/// A conversation as a message names it, for [`upsert`].
#[derive(Debug, Clone)]
pub struct NewConversation<'name> {
    pub connector_id: Uuid,
    pub key: &'name str,
    pub title: &'name str,
    pub created_by: Uuid,
}

#[derive(Insertable)]
#[diesel(table_name = conversations)]
struct ConversationRow<'name> {
    id: Uuid,
    connector_id: Uuid,
    key: &'name str,
    title: &'name str,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// The conversation `new` names, made when it is new, its title taken from `new` either way,
/// since its newest message names it best. Locks its row until the transaction ends.
///
/// # Errors
/// Propagates any database error, including a check violation for a key or title out of bounds.
pub async fn upsert(
    connection: &mut AsyncPgConnection,
    new: &NewConversation<'_>,
    now: DateTime<Utc>,
) -> QueryResult<Conversation> {
    diesel::insert_into(conversations::table)
        .values(ConversationRow {
            id: Uuid::now_v7(),
            connector_id: new.connector_id,
            key: new.key,
            title: new.title,
            created_by: new.created_by,
            created_at: now,
            updated_at: now,
        })
        .on_conflict((conversations::connector_id, conversations::key))
        .do_update()
        .set((
            conversations::title.eq(new.title),
            conversations::updated_at.eq(now),
        ))
        .returning(Conversation::as_returning())
        .get_result(connection)
        .await
}

/// The conversation `key` of connector `connector_id`, if it was ever gathered.
///
/// # Errors
/// Propagates any database error.
pub async fn find_by_key(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
    key: &str,
) -> QueryResult<Option<Conversation>> {
    conversations::table
        .filter(conversations::connector_id.eq(connector_id))
        .filter(conversations::key.eq(key))
        .select(Conversation::as_select())
        .first(connection)
        .await
        .optional()
}

/// Conversation `id`.
///
/// # Errors
/// `NotFound` when it was removed; any other database error.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Conversation> {
    conversations::table
        .find(id)
        .select(Conversation::as_select())
        .first(connection)
        .await
}

/// Conversation `id`, locked until the transaction ends, or `None` when another transaction
/// holds it or it was removed: whoever holds it is already working on it.
///
/// # Errors
/// Propagates any database error.
pub async fn lock_unless_held(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Option<Conversation>> {
    conversations::table
        .find(id)
        .select(Conversation::as_select())
        .for_update()
        .skip_locked()
        .first(connection)
        .await
        .optional()
}

/// Conversation `id`, locked until the transaction ends, waiting for whoever holds it.
///
/// # Errors
/// `NotFound` when it was removed; any other database error.
pub async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Conversation> {
    conversations::table
        .find(id)
        .select(Conversation::as_select())
        .for_update()
        .first(connection)
        .await
}

/// The conversation that gathered any of `record_ids`, if one did. A run's records are all one
/// conversation's, so whichever is found first answers for them all.
///
/// # Errors
/// Propagates any database error.
pub async fn of_records(
    connection: &mut AsyncPgConnection,
    record_ids: &[Uuid],
) -> QueryResult<Option<Conversation>> {
    conversation_messages::table
        .inner_join(conversations::table)
        .filter(conversation_messages::timeline_record_id.eq_any(record_ids))
        .select(Conversation::as_select())
        .first(connection)
        .await
        .optional()
}

/// A message a conversation gathers, for [`add_message`].
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = conversation_messages)]
pub struct NewMessage {
    pub timeline_record_id: Uuid,
    pub conversation_id: Uuid,
    pub wakes: bool,
    pub received_at: DateTime<Utc>,
}

/// Adds `new` to its conversation, waiting on a run and an agent. A record already gathered is
/// left as it is.
///
/// # Errors
/// Propagates any database error.
pub async fn add_message(connection: &mut AsyncPgConnection, new: NewMessage) -> QueryResult<()> {
    diesel::insert_into(conversation_messages::table)
        .values(new)
        .on_conflict_do_nothing()
        .execute(connection)
        .await?;
    Ok(())
}

/// Whether conversation `id` has a message no run has taken yet.
///
/// # Errors
/// Propagates any database error.
pub async fn has_untaken(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        conversation_messages::table
            .filter(conversation_messages::conversation_id.eq(id))
            .filter(conversation_messages::run_id.is_null()),
    ))
    .get_result(connection)
    .await
}

/// Every message no run has taken yet, of every conversation: what the engine looks at to
/// find the conversations that went quiet.
///
/// # Errors
/// Propagates any database error.
pub async fn untaken(connection: &mut AsyncPgConnection) -> QueryResult<Vec<ConversationMessage>> {
    conversation_messages::table
        .filter(conversation_messages::run_id.is_null())
        .select(ConversationMessage::as_select())
        .load(connection)
        .await
}

/// Conversation `id`'s messages no run has taken yet, oldest first, at most `limit`.
///
/// # Errors
/// Propagates any database error.
pub async fn untaken_of(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    limit: i64,
) -> QueryResult<Vec<ConversationMessage>> {
    conversation_messages::table
        .filter(conversation_messages::conversation_id.eq(id))
        .filter(conversation_messages::run_id.is_null())
        .order((
            conversation_messages::received_at.asc(),
            conversation_messages::timeline_record_id.asc(),
        ))
        .limit(limit)
        .select(ConversationMessage::as_select())
        .load(connection)
        .await
}

/// The records of conversation `id`'s messages a run took before `before`, newest first, at
/// most `limit`: what a run reads again as context for what it takes.
///
/// # Errors
/// Propagates any database error.
pub async fn taken_before(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    before: DateTime<Utc>,
    limit: i64,
) -> QueryResult<Vec<Uuid>> {
    conversation_messages::table
        .filter(conversation_messages::conversation_id.eq(id))
        .filter(conversation_messages::run_id.is_not_null())
        .filter(conversation_messages::received_at.le(before))
        .order((
            conversation_messages::received_at.desc(),
            conversation_messages::timeline_record_id.desc(),
        ))
        .limit(limit)
        .select(conversation_messages::timeline_record_id)
        .load(connection)
        .await
}

/// Marks `record_ids` as taken by run `run_id`.
///
/// # Errors
/// Propagates any database error.
pub async fn take(
    connection: &mut AsyncPgConnection,
    record_ids: &[Uuid],
    run_id: Uuid,
) -> QueryResult<()> {
    diesel::update(
        conversation_messages::table
            .filter(conversation_messages::timeline_record_id.eq_any(record_ids))
            .filter(conversation_messages::run_id.is_null()),
    )
    .set(conversation_messages::run_id.eq(run_id))
    .execute(connection)
    .await?;
    Ok(())
}

/// Conversation `id`'s messages no agent was given yet, oldest first, at most `limit`.
///
/// # Errors
/// Propagates any database error.
pub async fn undelivered_of(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    limit: i64,
) -> QueryResult<Vec<ConversationMessage>> {
    conversation_messages::table
        .filter(conversation_messages::conversation_id.eq(id))
        .filter(conversation_messages::delivered_at.is_null())
        .order((
            conversation_messages::received_at.asc(),
            conversation_messages::timeline_record_id.asc(),
        ))
        .limit(limit)
        .select(ConversationMessage::as_select())
        .load(connection)
        .await
}

/// What one of a handling agent's turns leaves on its conversation.
#[derive(Debug, Clone)]
pub struct TurnRecorded<'ids> {
    /// The session the turn left, which the next resumes.
    pub harness_session_id: Option<String>,
    /// The messages the turn was given.
    pub delivered: &'ids [Uuid],
    /// The wait the agent asked for and may take: until when, and in which run.
    pub wait: Option<(DateTime<Utc>, Uuid)>,
}

/// Records one handling turn on conversation `id`: its session, the messages it was given, and
/// the wait it asked for, ending any wait it was owed after. Call it in the transaction that
/// also writes what the turn decided, so a turn's effects land together or not at all.
///
/// # Errors
/// Propagates any database error.
pub async fn record_turn(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    turn: &TurnRecorded<'_>,
    now: DateTime<Utc>,
) -> QueryResult<()> {
    // A `None` in the tuple leaves its column as it is: a turn that left no session keeps the
    // one it resumed, and only a wait names the run that took it.
    diesel::update(conversations::table.find(id))
        .set((
            turn.harness_session_id
                .as_deref()
                .map(|session| conversations::harness_session_id.eq(session)),
            conversations::waiting_until.eq(turn.wait.map(|(until, _run_id)| until)),
            turn.wait
                .map(|(_until, run_id)| conversations::waited_in_run_id.eq(run_id)),
            conversations::updated_at.eq(now),
        ))
        .execute(connection)
        .await?;
    diesel::update(
        conversation_messages::table
            .filter(conversation_messages::timeline_record_id.eq_any(turn.delivered))
            .filter(conversation_messages::delivered_at.is_null()),
    )
    .set(conversation_messages::delivered_at.eq(now))
    .execute(connection)
    .await?;
    Ok(())
}
