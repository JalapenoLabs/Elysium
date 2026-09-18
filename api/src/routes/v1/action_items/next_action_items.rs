// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/action-items/next`: Next, in order, and how many items wait in the inbox.
//!
//! Clients show the first item and lead with a triage card while the inbox is not empty.
//! The order is `crate::action_items::next`.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use chrono::Utc;
use serde_json::{Value, json};

use super::item_responses;
use crate::errors::ApiError;
use crate::models::action_item;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (items, inbox_count) = action_item::next(&mut connection, Utc::now()).await?;
    let items = item_responses(&mut connection, items).await?;

    Ok(Json(json!({ "items": items, "inboxCount": inbox_count })))
}
