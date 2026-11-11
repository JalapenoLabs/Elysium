// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}/manager-assignment`: a person takes an item back from its lead
//! project's Project Manager. Its wake goes if no turn has taken it; batches already planned stay,
//! each the person's to decide (`crate::models::action_item_assignment`).

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_item_write;
use crate::action_items::Actor;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::action_item_assignment;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let unassigned =
        action_item_assignment::unassign(&mut connection, id, Actor::User(member.id()), now)
            .await?;
    let item = publish_item_write(&state.events, &mut connection, unassigned, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
