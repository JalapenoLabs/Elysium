// Copyright © 2026 Jalapeno Labs

//! `/api/v1/changesets`: the changes Elysia and coding agents propose, waiting for the user.
//!
//! The user reviews a changeset, approves or rejects each operation, applies it, and can
//! undo it afterwards. Nothing a changeset proposes is written until it is applied. Every
//! write publishes the changeset, and applying and undoing publish everything they changed,
//! the way the user's own writes do. See `docs/action-items.md`.

mod apply_changeset;
mod decide_changeset;
mod get_changeset;
mod list_changesets;
mod undo_changeset;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::action_items::changesets::{self, Operation};
use crate::errors::ApiError;
use crate::models::changeset::{
    ChangesetDecision, ChangesetOperation, ChangesetOutcome, ChangesetState, Staged, Touched,
};
use crate::models::{action_item, initiative};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::action_items::{
    CommentResponse, item_responses, publish_history, publish_initiatives, publish_item_links,
};
use crate::routes::v1::initiatives::publish_member_items;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_changesets::handle))
        .route("/{id}", get(get_changeset::handle))
        .route("/{id}/decide", post(decide_changeset::handle))
        .route("/{id}/apply", post(apply_changeset::handle))
        .route("/{id}/undo", post(undo_changeset::handle))
}

/// A changeset as clients see it, with its operations in order.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangesetResponse {
    id: Uuid,
    /// `elysia` or `session:<number>`.
    proposer: String,
    /// The project a coding session proposed it in.
    project_id: Option<Uuid>,
    summary: String,
    state: ChangesetState,
    /// When it was applied or rejected.
    decided_at: Option<DateTime<Utc>>,
    undone_at: Option<DateTime<Utc>>,
    operations: Vec<OperationResponse>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

/// One operation as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationResponse {
    id: Uuid,
    /// From 1, in the order operations apply.
    position: i32,
    /// The change: an object whose `kind` says what it does.
    operation: Value,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
    /// The positions of the earlier operations whose records this one acts on.
    depends_on: Vec<u32>,
    decision: ChangesetDecision,
    outcome: ChangesetOutcome,
    /// Why it failed or was skipped.
    error: Option<String>,
    /// What applying it did: the ids it acted on or created.
    result: Value,
    /// What undoing it did and could not do; `null` until the changeset is undone.
    undo: Option<Value>,
}

impl From<Staged> for ChangesetResponse {
    fn from(staged: Staged) -> Self {
        let Staged {
            changeset,
            operations,
        } = staged;
        let parsed: Vec<Operation> = operations.iter().map(ChangesetOperation::parsed).collect();
        let dependencies = changesets::dependencies(&parsed);
        Self {
            operations: operations
                .into_iter()
                .zip(dependencies)
                .map(|(row, depends_on)| OperationResponse {
                    id: row.id,
                    position: row.position,
                    operation: row.operation,
                    reason: row.reason,
                    quote: row.quote,
                    source: row.source,
                    depends_on,
                    decision: row.decision,
                    outcome: row.outcome,
                    error: row.error,
                    result: row.result,
                    undo: row.undo,
                })
                .collect(),
            id: changeset.id,
            proposer: changeset.proposer,
            project_id: changeset.project_id,
            summary: changeset.summary,
            state: changeset.state,
            decided_at: changeset.decided_at,
            undone_at: changeset.undone_at,
            created_at: changeset.created_at,
            updated_at: changeset.updated_at,
        }
    }
}

/// Publishes the changeset and answers it as clients see it.
pub fn publish_changeset(events: &EventBus, staged: Staged) -> ChangesetResponse {
    let response = ChangesetResponse::from(staged);
    events.publish(&ServerEvent::ChangesetUpserted(response.clone()));
    response
}

/// Publishes everything applying or undoing a changeset changed, as the user's own writes
/// would: the history, the comments and links, every item as it ended up, and every
/// initiative whose progress or membership moved.
///
/// # Errors
/// Propagates any database error.
async fn publish_touched(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    touched: Touched,
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let Touched {
        history,
        item_ids,
        mut initiative_ids,
        link_item_ids,
        comments,
        withdrawn_comments,
        removed_links,
    } = touched;

    publish_history(events, history);
    for comment in comments {
        events.publish(&ServerEvent::ActionItemCommentUpserted(
            CommentResponse::from(comment),
        ));
    }
    for comment in withdrawn_comments {
        events.publish(&ServerEvent::ActionItemCommentDeleted {
            id: comment.id,
            action_item_id: comment.action_item_id,
        });
    }
    for link in removed_links {
        events.publish(&ServerEvent::ActionItemLinkDeleted {
            id: link.id,
            action_item_id: link.action_item_id,
        });
    }

    let mut live_items = Vec::new();
    for item_id in item_ids {
        let item = action_item::find(connection, item_id).await?;
        initiative_ids.extend(action_item::current_initiative_ids(connection, item_id).await?);
        if item.deleted_at.is_some() {
            events.publish(&ServerEvent::ActionItemDeleted { id: item_id });
        } else {
            live_items.push(item);
        }
    }
    for response in item_responses(connection, live_items).await? {
        events.publish(&ServerEvent::ActionItemUpserted(response));
    }
    for item_id in link_item_ids {
        publish_item_links(events, connection, item_id).await?;
    }

    let initiative_ids: Vec<Uuid> = initiative_ids.into_iter().collect();
    publish_initiatives(events, connection, &initiative_ids, now).await?;
    for initiative_id in initiative_ids {
        let found = initiative::find(connection, initiative_id).await?;
        // Items list only live initiatives, so one deleted takes itself off its members.
        if found.deleted_at.is_some() {
            events.publish(&ServerEvent::InitiativeDeleted { id: initiative_id });
            publish_member_items(events, connection, initiative_id, now).await?;
        }
    }
    Ok(())
}
