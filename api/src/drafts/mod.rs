// Copyright © 2026 Jalapeno Labs

//! Sending as the person: drafts, and the one way any message leaves Elysium in someone's
//! name. See `docs/connectors.md`, Sending.
//!
//! # A draft, then a send
//!
//! Everything sent is first a draft (`crate::models::draft`): what it says, and where it goes,
//! decided when it is written. A reply to a timeline record goes through the connector the record
//! came from when its kind sends ([`kinds::Replies::Connector`]: Slack, Teams), or by mail from a
//! mailbox of the person's own when its kind is answered by mail ([`kinds::Replies::Mailbox`]:
//! Outlook, Gmail). A new mail names its mailbox and recipients.
//!
//! [`send`] is the entry point every send goes through, the person pressing Send in the web app
//! and, later, a follow-up they approve. Sending is always the person's own act, so it needs no
//! changeset; something that is not the person can only write a draft for them to send.
//!
//! # What a send leaves behind
//!
//! A send that succeeds becomes a timeline record of the person's, named as the kind's reader
//! names what it reads, so reading the message back later records nothing twice. A send that
//! fails keeps the draft, its text untouched, with the reason in `last_error`, and can be sent
//! again. A send is never retried on its own: posting to a person twice is worse than not
//! posting, and a send that timed out may still have gone out, which the reason says.

use std::time::Duration;

use anyhow::Context;
use chrono::{TimeDelta, Utc};
use diesel_async::AsyncConnection;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use crate::connectors::Outgoing;
use crate::connectors::kinds::{self, Replies};
use crate::errors::ApiError;
use crate::mail::reply::{self as mail_reply, MailTarget};
use crate::mail::transport::{self, Envelope};
use crate::models::connector;
use crate::models::draft::{
    self, BODY_MAX_CHARACTERS, Draft, DraftState, NewDraft, ReplyPlace, Sender,
};
use crate::models::mail_account::{self, MailAccount};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::models::user::User;
use crate::realtime::ServerEvent;
use crate::routes::v1::mail::open_mailbox;
use crate::state::AppState;

/// The longest one send may take, from claiming the draft to the provider's answer. Inside the
/// API's 30 second request timeout, so the person always hears how it ended, and longer than
/// each page script's own 15 second wait and SMTP's 15 second exchange, so those answer first
/// with their own reasons.
const SEND_TIMEOUT: Duration = Duration::from_secs(20);

/// How long a draft left `sending` waits before it may be sent again. Only a process that
/// stopped mid-send leaves one, so this is comfortably past [`SEND_TIMEOUT`].
const SENDING_STALE_AFTER: TimeDelta = TimeDelta::minutes(2);

/// What the reason says when a send ran out of time: it may have gone out.
const TIMED_OUT: &str = "the send did not finish in time, and it may still have gone out; check \
                         before sending it again";

/// A reply to one of the person's records.
#[derive(Debug, Clone)]
pub struct Reply {
    pub record_id: Uuid,
    /// Where in a chat conversation it goes; a reply by mail takes none.
    pub place: Option<ReplyPlace>,
    /// The mailbox a reply by mail is sent from; optional when the person has exactly one.
    pub mail_account_id: Option<Uuid>,
    pub body: String,
}

/// A new mail, from one of the person's mailboxes.
#[derive(Debug, Clone)]
pub struct NewMail {
    pub mail_account_id: Uuid,
    pub to: Vec<String>,
    pub subject: String,
    pub body: String,
}

/// Writes `person`'s reply to one of their records as a draft, through what answers the record's
/// kind. Nothing is sent until [`send`].
///
/// # Errors
/// [`ApiError::NotFound`] for a record or mailbox that is not the person's,
/// [`ApiError::BadRequest`] for a record nothing can answer or a body out of bounds, and
/// [`ApiError::CodedBadRequest`] when a reply by mail has no mailbox to go from.
pub async fn draft_reply(state: &AppState, person: &User, reply: Reply) -> Result<Draft, ApiError> {
    check_body(&reply.body)?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let record = timeline_record::find_owned_record(&mut connection, reply.record_id, person.id)
        .await?
        .ok_or(ApiError::NotFound)?;
    let connector_id = record.connector_id.ok_or_else(|| {
        ApiError::BadRequest(
            "this record did not come from a connector, so it cannot be answered".to_owned(),
        )
    })?;
    let source = connector::find(&mut connection, connector_id).await?;
    if !kinds::is_visible(&source, person.id) {
        return Err(ApiError::NotFound);
    }
    let kind = kinds::kind_of(&source)?;

    let (sender, target, place) = match kinds::definition(kind).replies {
        Replies::Never => {
            return Err(ApiError::BadRequest(format!(
                "{} records cannot be answered from Elysium",
                kind.as_str()
            )));
        }
        Replies::Connector { places } => {
            let place = reply
                .place
                .or_else(|| places.first().copied())
                .ok_or_else(|| {
                    ApiError::BadRequest("say where in the conversation the reply goes".to_owned())
                })?;
            let target = kinds::reply_target(kind, &record, place)?;
            (Sender::Connector(source.id), target, Some(place))
        }
        Replies::Mailbox => {
            let mailbox = sending_mailbox(&mut connection, person, reply.mail_account_id).await?;
            let target = mail_reply::reply_target(&record).map_err(ApiError::BadRequest)?;
            (Sender::Mailbox(mailbox.id), target_value(&target)?, None)
        }
    };

    let created = draft::create(
        &mut connection,
        NewDraft {
            reply_to_record_id: Some(record.id),
            sender,
            target,
            place,
            body: reply.body,
            created_by: person.id,
        },
    )
    .await?;
    announce(state, &created);
    Ok(created)
}

/// Writes `person`'s new mail as a draft, from one of their mailboxes. Nothing is sent until
/// [`send`].
///
/// # Errors
/// [`ApiError::NotFound`] for a mailbox that is not the person's, and
/// [`ApiError::BadRequest`] for a recipient that is not an address or a body out of bounds.
pub async fn draft_mail(state: &AppState, person: &User, mail: NewMail) -> Result<Draft, ApiError> {
    check_body(&mail.body)?;
    let target = mail_reply::new_target(&mail.to, &mail.subject).map_err(ApiError::BadRequest)?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let mailbox = sending_mailbox(&mut connection, person, Some(mail.mail_account_id)).await?;
    let created = draft::create(
        &mut connection,
        NewDraft {
            reply_to_record_id: None,
            sender: Sender::Mailbox(mailbox.id),
            target: target_value(&target)?,
            place: None,
            body: mail.body,
            created_by: person.id,
        },
    )
    .await?;
    announce(state, &created);
    Ok(created)
}

/// Replaces the text of `person`'s draft `id`, which keeps where it goes.
///
/// # Errors
/// [`ApiError::NotFound`] for a draft that is not theirs, [`ApiError::Conflict`] while it is
/// being sent or once it was, and [`ApiError::BadRequest`] for a body out of bounds.
pub async fn update(
    state: &AppState,
    person: &User,
    id: Uuid,
    body: &str,
) -> Result<Draft, ApiError> {
    check_body(body)?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Some(updated) = draft::update_body(&mut connection, id, person.id, body).await? else {
        let existing = draft::find_owned(&mut connection, id, person.id).await?;
        return Err(not_now(existing.state));
    };
    announce(state, &updated);
    Ok(updated)
}

/// Throws away `person`'s draft `id`. A sent draft's record stays on the timeline.
///
/// # Errors
/// [`ApiError::NotFound`] for a draft that is not theirs, and [`ApiError::Conflict`] while it is
/// being sent.
pub async fn discard(state: &AppState, person: &User, id: Uuid) -> Result<(), ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    if !draft::delete_unless_sending(&mut connection, id, person.id).await? {
        let existing = draft::find_owned(&mut connection, id, person.id).await?;
        return Err(not_now(existing.state));
    }
    state.events.publish(&ServerEvent::DraftChanged {
        id,
        created_by: person.id,
    });
    Ok(())
}

/// Sends `person`'s draft `id`, as them, and answers it as the send left it: `sent`, with the
/// record it became, or `failed`, with why and its text untouched. A failed send is an answer,
/// not an error.
///
/// # Errors
/// [`ApiError::NotFound`] for a draft that is not theirs, [`ApiError::Conflict`] while another
/// send of it is under way or once it was sent, and [`ApiError::Internal`] when the outcome
/// cannot be recorded.
pub async fn send(state: &AppState, person: &User, id: Uuid) -> Result<Draft, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let now = Utc::now();
    let Some(claimed) =
        draft::claim(&mut connection, id, person.id, now, SENDING_STALE_AFTER).await?
    else {
        let existing = draft::find_owned(&mut connection, id, person.id).await?;
        return Err(not_now(existing.state));
    };
    // The send takes seconds; no pooled connection waits on it.
    drop(connection);
    announce(state, &claimed);

    let delivered = match tokio::time::timeout(SEND_TIMEOUT, deliver(state, person, &claimed)).await
    {
        Ok(delivered) => delivered,
        Err(_elapsed) => Err(TIMED_OUT.to_owned()),
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let finished = match delivered {
        Ok(record) => {
            let (finished, created) = record_sent(&mut connection, &claimed, record).await?;
            if let Some(created) = created {
                state.events.publish(&ServerEvent::TimelineRecordCreated {
                    id: created.id,
                    source_kind: created.source_kind,
                    created_by: created.created_by,
                    action_item_id: None,
                    conversation_id: None,
                });
            }
            finished
        }
        Err(reason) => {
            event!(
                name: "drafts.send.failed",
                Level::WARN,
                draft.id = %claimed.id,
                error.message = %reason,
                "a draft could not be sent: {{error.message}}",
            );
            draft::mark_failed(&mut connection, claimed.id, &reason).await?
        }
    };
    announce(state, &finished);
    Ok(finished)
}

/// Sends `claimed` through its sender, and answers the record it becomes, or why it was not sent
/// in words its person can act on.
async fn deliver(
    state: &AppState,
    person: &User,
    claimed: &Draft,
) -> Result<NewTimelineRecord, String> {
    let mut connection = state
        .database
        .get()
        .await
        .map_err(|error| format!("Elysium could not reach its database: {error}"))?;
    let answered = match claimed.reply_to_record_id {
        Some(record_id) => {
            timeline_record::find_owned_record(&mut connection, record_id, person.id)
                .await
                .map_err(|error| {
                    format!("Elysium could not read the message it answers: {error}")
                })?
        }
        None => None,
    };

    match claimed.sender() {
        Sender::Connector(connector_id) => {
            let through = connector::find(&mut connection, connector_id)
                .await
                .map_err(|_missing| {
                    "the connector this was to go through was removed".to_owned()
                })?;
            drop(connection);
            let sent = kinds::send(
                state,
                &through,
                Outgoing {
                    target: &claimed.target,
                    body: &claimed.body,
                    sender_name: display_name(person),
                },
            )
            .await
            .map_err(|error| error.to_string())?;
            Ok(NewTimelineRecord {
                id: Uuid::now_v7(),
                source_kind: through.kind.clone(),
                connector_id: Some(through.id),
                external_id: Some(sent.external_id),
                occurred_at: sent.occurred_at,
                actor: display_name(person).to_owned(),
                title: answered.map_or_else(|| through.kind.clone(), |record| record.title),
                body: claimed.body.clone(),
                url: sent.url,
                media: None,
                metadata: with_draft(sent.metadata, claimed.id),
                created_by: person.id,
            })
        }
        Sender::Mailbox(mail_account_id) => {
            let account = mail_account::find(&mut connection, mail_account_id)
                .await
                .map_err(|_missing| "the mailbox this was to go from was removed".to_owned())?;
            drop(connection);
            if account.created_by != person.id {
                return Err("only the person who connected this mailbox sends from it".to_owned());
            }
            let target: MailTarget = serde_json::from_value(claimed.target.clone())
                .map_err(|error| format!("this draft's recipients cannot be read: {error}"))?;
            let mailbox = open_mailbox(state, &account)
                .await
                .map_err(|error| error.to_string())?;
            let envelope = Envelope {
                to: target.to.clone(),
                subject: target.subject.clone(),
                body: claimed.body.clone(),
                in_reply_to: target.in_reply_to.clone(),
                references: target.references.clone(),
            };
            let message_id = transport::send(&mailbox, &envelope)
                .await
                .map_err(|error| error.to_string())?;
            Ok(NewTimelineRecord {
                id: Uuid::now_v7(),
                source_kind: MAIL_SOURCE_KIND.to_owned(),
                connector_id: None,
                external_id: Some(message_id.clone()),
                occurred_at: Utc::now(),
                actor: format!("{} <{}>", display_name(person), account.address),
                title: target.subject,
                body: claimed.body.clone(),
                url: None,
                media: None,
                metadata: with_draft(
                    json!({
                        "mailAccountId": account.id,
                        "to": target.to,
                        "messageId": message_id,
                        "inReplyTo": target.in_reply_to,
                    }),
                    claimed.id,
                ),
                created_by: person.id,
            })
        }
    }
}

/// What a mail sent from a mailbox is recorded as, since no connector read it.
pub const MAIL_SOURCE_KIND: &str = "mail";

/// Stores the record a sent draft became and marks it sent, together, and answers the draft
/// and the record when it is new. A reader that already read the message back keeps its own
/// record, which the draft then points at.
async fn record_sent(
    connection: &mut diesel_async::AsyncPgConnection,
    claimed: &Draft,
    record: NewTimelineRecord,
) -> Result<(Draft, Option<TimelineRecord>), ApiError> {
    let now = Utc::now();
    let draft_id = claimed.id;
    let stored = connection
        .transaction(async move |connection| {
            let connector_id = record.connector_id;
            let external_id = record.external_id.clone();
            let created = timeline_record::insert_new(connection, record).await?;
            let record_id = match (&created, connector_id, external_id) {
                (Some(created), _, _) => created.id,
                (None, Some(connector_id), Some(external_id)) => {
                    timeline_record::find_by_external_id(connection, connector_id, &external_id)
                        .await?
                        .map(|existing| existing.id)
                        .ok_or(diesel::result::Error::NotFound)?
                }
                // Only a reader can have recorded the message first, and a mail sent from a
                // mailbox has no reader and a new Message-ID, so this answers nothing it could.
                (None, _, _) => return Err(diesel::result::Error::NotFound),
            };
            let sent = draft::mark_sent(connection, draft_id, record_id, now).await?;
            Ok::<_, diesel::result::Error>((sent, created))
        })
        .await?;
    Ok(stored)
}

/// The mailbox `person` sends from: the one they named, or their only one.
///
/// # Errors
/// [`ApiError::NotFound`] for a mailbox that is not theirs, [`ApiError::BadRequest`] for one that
/// is turned off, and [`ApiError::CodedBadRequest`] when they named none and have none, or
/// several.
async fn sending_mailbox(
    connection: &mut diesel_async::AsyncPgConnection,
    person: &User,
    named: Option<Uuid>,
) -> Result<MailAccount, ApiError> {
    let mailbox = if let Some(id) = named {
        let mailbox = mail_account::find(connection, id).await?;
        if mailbox.created_by != person.id {
            return Err(ApiError::NotFound);
        }
        mailbox
    } else {
        let mut own: Vec<MailAccount> = mail_account::list(connection)
            .await?
            .into_iter()
            .filter(|mailbox| mailbox.created_by == person.id && mailbox.is_active)
            .collect();
        match own.len() {
            0 => {
                return Err(ApiError::CodedBadRequest {
                    code: "no_mailbox",
                    message: "connect a mailbox under Settings, Email to reply by mail".to_owned(),
                });
            }
            1 => own.remove(0),
            _ => {
                return Err(ApiError::CodedBadRequest {
                    code: "mailbox_required",
                    message: "name the mailbox to send from".to_owned(),
                });
            }
        }
    };
    if !mailbox.is_active {
        return Err(ApiError::BadRequest(format!(
            "{} is turned off; turn it on under Settings, Email to send from it",
            mailbox.address
        )));
    }
    Ok(mailbox)
}

/// Refuses a body that is empty or longer than a draft keeps.
fn check_body(body: &str) -> Result<(), ApiError> {
    if body.trim().is_empty() {
        return Err(ApiError::BadRequest("write something to send".to_owned()));
    }
    if body.chars().count() > BODY_MAX_CHARACTERS {
        return Err(ApiError::BadRequest(format!(
            "a message is at most {BODY_MAX_CHARACTERS} characters"
        )));
    }
    Ok(())
}

/// Why a draft in `state` cannot be changed or sent right now.
fn not_now(state: DraftState) -> ApiError {
    match state {
        DraftState::Sending => ApiError::Conflict("this draft is being sent"),
        DraftState::Sent => ApiError::Conflict("this draft was already sent"),
        DraftState::Draft | DraftState::Failed => {
            ApiError::Conflict("this draft changed meanwhile; try again")
        }
    }
}

fn target_value(target: &MailTarget) -> Result<Value, ApiError> {
    serde_json::to_value(target).map_err(|error| ApiError::Internal(error.into()))
}

/// The person's name as a message shows its sender, or their address without one.
fn display_name(person: &User) -> &str {
    let name = person.name.trim();
    if !name.is_empty() {
        return name;
    }
    person.email.as_deref().unwrap_or("Elysium")
}

/// `metadata` with the draft it was sent from, so the timeline can tell what the person sent.
fn with_draft(mut metadata: Value, draft_id: Uuid) -> Value {
    if !metadata.is_object() {
        metadata = json!({});
    }
    metadata["draftId"] = json!(draft_id);
    metadata
}

fn announce(state: &AppState, draft: &Draft) {
    state.events.publish(&ServerEvent::DraftChanged {
        id: draft.id,
        created_by: draft.created_by,
    });
}

#[cfg(test)]
mod tests;
