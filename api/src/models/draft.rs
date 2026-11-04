// Copyright © 2026 Jalapeno Labs

//! Drafts: messages a person sends as themselves, kept from the moment they are written until
//! long after they are sent. See `crate::drafts` and `docs/connectors.md`, Sending.
//!
//! A draft goes through exactly one sender, a connector or a mailbox
//! (`drafts_one_sender`), to a target in that sender's own terms. Its state moves
//! `draft` (or `failed`) to `sending` to `sent` or `failed`, and only [`claim`] moves it to
//! `sending`, in one statement, so two presses of Send never post one message twice. A draft
//! that failed keeps its text and the reason (`last_error`) and can be sent again.

use chrono::{DateTime, TimeDelta, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::drafts;

/// The longest draft, in characters, which `drafts_body_length` holds too. Slack takes 40,000
/// and Teams about 28,000; this stays under both, with room for what escaping adds.
pub const BODY_MAX_CHARACTERS: usize = 20_000;

/// The longest reason a failed send keeps, which `drafts_error_length` holds too.
pub const ERROR_MAX_CHARACTERS: usize = 2000;

/// Where a chat reply goes in its conversation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ReplyPlace"]
#[serde(rename_all = "kebab-case")]
pub enum ReplyPlace {
    /// In the thread of the message it answers.
    Thread,
    /// A new message in the conversation the answered message was said in.
    Conversation,
}

/// How far a draft got.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::DraftState"]
#[serde(rename_all = "kebab-case")]
pub enum DraftState {
    /// Written and never sent.
    Draft,
    /// A send is under way.
    Sending,
    Sent,
    /// The last send failed; `last_error` says why, and it can be sent again.
    Failed,
}

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = drafts, check_for_backend(diesel::pg::Pg))]
pub struct Draft {
    pub id: Uuid,
    /// The record it answers, for a reply.
    pub reply_to_record_id: Option<Uuid>,
    /// The connector that sends it, for a chat reply.
    pub connector_id: Option<Uuid>,
    /// The mailbox that sends it, for mail.
    pub mail_account_id: Option<Uuid>,
    /// Where it goes, in its sender's own terms.
    pub target: Value,
    pub place: Option<ReplyPlace>,
    pub body: String,
    pub state: DraftState,
    pub last_error: Option<String>,
    pub attempted_at: Option<DateTime<Utc>>,
    pub sent_at: Option<DateTime<Utc>>,
    /// The record the sent message became.
    pub sent_record_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// What sends a draft. Exactly one of the two columns it fills is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sender {
    Connector(Uuid),
    Mailbox(Uuid),
}

impl Draft {
    /// What sends it, as `drafts_one_sender` holds there is exactly one.
    ///
    /// # Panics
    /// When neither sender is set, which the constraint makes impossible.
    pub fn sender(&self) -> Sender {
        match (self.connector_id, self.mail_account_id) {
            (Some(connector_id), _) => Sender::Connector(connector_id),
            (None, Some(mail_account_id)) => Sender::Mailbox(mail_account_id),
            (None, None) => panic!("drafts_one_sender holds that a draft has a sender"),
        }
    }
}

/// A draft to store.
#[derive(Debug, Clone)]
pub struct NewDraft {
    pub reply_to_record_id: Option<Uuid>,
    pub sender: Sender,
    pub target: Value,
    pub place: Option<ReplyPlace>,
    pub body: String,
    pub created_by: Uuid,
}

/// Stores a new draft, unsent.
///
/// # Errors
/// Propagates any database error, including a check violation for a body out of bounds.
pub async fn create(connection: &mut AsyncPgConnection, new: NewDraft) -> QueryResult<Draft> {
    let (connector_id, mail_account_id) = match new.sender {
        Sender::Connector(id) => (Some(id), None),
        Sender::Mailbox(id) => (None, Some(id)),
    };
    diesel::insert_into(drafts::table)
        .values((
            drafts::id.eq(Uuid::now_v7()),
            drafts::reply_to_record_id.eq(new.reply_to_record_id),
            drafts::connector_id.eq(connector_id),
            drafts::mail_account_id.eq(mail_account_id),
            drafts::target.eq(new.target),
            drafts::place.eq(new.place),
            drafts::body.eq(new.body),
            drafts::created_by.eq(new.created_by),
        ))
        .returning(Draft::as_returning())
        .get_result(connection)
        .await
}

/// The draft `id`, when `owner` wrote it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or is someone else's.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<Draft> {
    drafts::table
        .filter(drafts::id.eq(id))
        .filter(drafts::created_by.eq(owner))
        .select(Draft::as_select())
        .first(connection)
        .await
}

/// `owner`'s drafts answering `record_id` that were never sent, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_unsent_for_record(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    owner: Uuid,
) -> QueryResult<Vec<Draft>> {
    drafts::table
        .filter(drafts::reply_to_record_id.eq(record_id))
        .filter(drafts::created_by.eq(owner))
        .filter(drafts::state.ne(DraftState::Sent))
        .order((drafts::created_at.desc(), drafts::id.desc()))
        .select(Draft::as_select())
        .load(connection)
        .await
}

/// Replaces the text of `owner`'s draft `id` while it is not being sent and was never sent.
/// Answers `None` when there is no such draft in a state that allows it.
///
/// # Errors
/// Propagates any database error, including a check violation for a body out of bounds.
pub async fn update_body(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
    body: &str,
) -> QueryResult<Option<Draft>> {
    diesel::update(
        drafts::table
            .filter(drafts::id.eq(id))
            .filter(drafts::created_by.eq(owner))
            .filter(drafts::state.eq_any([DraftState::Draft, DraftState::Failed])),
    )
    .set(drafts::body.eq(body))
    .returning(Draft::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Removes `owner`'s draft `id` unless it is being sent. Answers whether it removed one.
///
/// # Errors
/// Propagates any database error.
pub async fn delete_unless_sending(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<bool> {
    let removed = diesel::delete(
        drafts::table
            .filter(drafts::id.eq(id))
            .filter(drafts::created_by.eq(owner))
            .filter(drafts::state.ne(DraftState::Sending)),
    )
    .execute(connection)
    .await?;
    Ok(removed > 0)
}

/// Moves `owner`'s draft `id` to `sending`, in one statement, when it may be sent: never sent,
/// or failed, or a send that started more than `stale_after` ago and never finished, which only
/// a process that stopped mid-send leaves behind. Answers `None` when it may not be sent now.
///
/// # Errors
/// Propagates any database error.
pub async fn claim(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
    now: DateTime<Utc>,
    stale_after: TimeDelta,
) -> QueryResult<Option<Draft>> {
    let unfinished_before = now - stale_after;
    diesel::update(
        drafts::table
            .filter(drafts::id.eq(id))
            .filter(drafts::created_by.eq(owner))
            .filter(
                drafts::state
                    .eq_any([DraftState::Draft, DraftState::Failed])
                    .or(drafts::state
                        .eq(DraftState::Sending)
                        .and(drafts::attempted_at.lt(unfinished_before))),
            ),
    )
    .set((
        drafts::state.eq(DraftState::Sending),
        drafts::attempted_at.eq(now),
        drafts::last_error.eq(None::<String>),
    ))
    .returning(Draft::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Records that draft `id` was sent and became `record_id`.
///
/// # Errors
/// Propagates any database error.
pub async fn mark_sent(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    record_id: Uuid,
    now: DateTime<Utc>,
) -> QueryResult<Draft> {
    diesel::update(drafts::table.find(id))
        .set((
            drafts::state.eq(DraftState::Sent),
            drafts::sent_at.eq(now),
            drafts::sent_record_id.eq(record_id),
            drafts::last_error.eq(None::<String>),
        ))
        .returning(Draft::as_returning())
        .get_result(connection)
        .await
}

/// Records that sending draft `id` failed, and why. The text stays, so it can be sent again.
///
/// # Errors
/// Propagates any database error.
pub async fn mark_failed(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    reason: &str,
) -> QueryResult<Draft> {
    let reason: String = reason.chars().take(ERROR_MAX_CHARACTERS).collect();
    diesel::update(drafts::table.find(id))
        .set((
            drafts::state.eq(DraftState::Failed),
            drafts::last_error.eq(reason),
        ))
        .returning(Draft::as_returning())
        .get_result(connection)
        .await
}
