// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/action-items/{id}/manager-assignment`: a person assigns an item to its lead
//! project's Project Manager, which wakes to plan it. It starts no worker: what the Project Manager
//! plans is a proposed batch the person approves (`crate::models::action_item_assignment`).
//!
//! Only the company's members assign, since a Project Manager's turns quote what was ingested,
//! which its routes answer the members alone.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_item_write;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::action_item_assignment::{self, Assigner};
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

    let assigned =
        action_item_assignment::assign(&mut connection, id, Assigner::person(member.id()), now)
            .await?;
    let item = publish_item_write(&state.events, &mut connection, assigned, &[], now).await?;

    Ok(Json(json!({ "item": item })))
}
