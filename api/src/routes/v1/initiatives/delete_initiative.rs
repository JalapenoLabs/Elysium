// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/initiatives/{id}`: hide an initiative everywhere until it is restored.
//!
//! Deleting is soft. Its items stay, and stay members, but stop listing it until it is
//! restored.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use super::{publish_initiative_write, publish_member_items};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::initiative;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let deleted = initiative::soft_delete(&mut connection, id, Actor::User, now).await?;
    publish_initiative_write(&state.events, &mut connection, deleted, now).await?;
    publish_member_items(&state.events, &mut connection, id, now).await?;

    Ok(StatusCode::NO_CONTENT)
}
