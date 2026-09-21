// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/changesets/{id}/undo`: reverse every operation an applied changeset
//! applied, last first, as the user.
//!
//! What cannot be reversed is recorded on each operation's `undo`: a comment already posted
//! to its provider stays there, an issue already closed stays closed, and a field or state
//! the user changed since keeps the user's value. See `crate::models::changeset::undo`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{publish_changeset, publish_touched};
use crate::errors::ApiError;
use crate::models::changeset::{self, Written};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Written { staged, touched } = changeset::undo(&mut connection, id, now).await?;
    publish_touched(&state.events, &mut connection, touched, now).await?;
    // Returning a resolved item leaves closes it owed for the watcher to drop.
    state.links.wake_watcher();
    let changeset = publish_changeset(&state.events, staged);

    Ok(Json(json!({ "changeset": changeset })))
}
