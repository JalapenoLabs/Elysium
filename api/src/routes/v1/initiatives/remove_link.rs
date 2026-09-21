// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/initiatives/{id}/links/{link_id}`: unlink a container. The items it
//! brought into the initiative leave it, each recording that; the items themselves stay,
//! with their links. An item that was also added by hand stays in the initiative.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::initiative_link;
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::{item_responses, publish_history, publish_initiatives};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((id, link_id)) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let unlinked = initiative_link::remove(&mut connection, id, link_id, Actor::User, now).await?;
    publish_history(&state.events, unlinked.history);
    state.events.publish(&ServerEvent::InitiativeLinkDeleted {
        id: link_id,
        initiative_id: id,
    });
    for item in item_responses(&mut connection, unlinked.items).await? {
        state.events.publish(&ServerEvent::ActionItemUpserted(item));
    }
    publish_initiatives(&state.events, &mut connection, &[id], now).await?;

    Ok(StatusCode::NO_CONTENT)
}
