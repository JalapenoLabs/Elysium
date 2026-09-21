// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/changesets/{id}/apply`: write the approved operations of a pending
//! changeset whose operations are all decided.
//!
//! Each link operation's target is read through its credential first, as a link the user
//! adds is. Then the approved operations apply in order, Elysium's data first: one that
//! cannot (its item deleted since, its target unreadable) is marked failed with the reason,
//! the operations depending on it are skipped, and the rest apply. The provider writes they
//! owe, such as closing a resolved item's issues, are landed by the watcher, which this wakes.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{publish_changeset, publish_touched};
use crate::action_items::changesets;
use crate::errors::ApiError;
use crate::models::changeset::{self, Written};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let staged = changeset::find(&mut connection, id).await?;
    // Reading a provider can take seconds; no connection is held while it does.
    drop(connection);
    let reads = changesets::read_link_targets(&state.links, &staged.operations).await;

    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Written { staged, touched } = changeset::apply(&mut connection, id, reads, now).await?;
    publish_touched(&state.events, &mut connection, touched, now).await?;
    state.links.wake_watcher();
    let changeset = publish_changeset(&state.events, staged);

    Ok(Json(json!({ "changeset": changeset })))
}
