// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/studio-items`: the grid. `?deleted=true` lists softly deleted items instead
//! of live ones.
//!
//! Each item's sessions come with it, so a tile shows its thread's state without a second
//! request.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{StudioItemResponse, session_responses};
use crate::errors::ApiError;
use crate::models::studio_item::{self, Liveness};
use crate::models::{coding_session, studio_asset};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default)]
    deleted: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let liveness = if query.deleted {
        Liveness::Deleted
    } else {
        Liveness::Live
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let items = studio_item::list(&mut connection, liveness).await?;
    let item_ids: Vec<_> = items.iter().map(|item| item.id).collect();
    let assets = studio_asset::list_for_items(&mut connection, &item_ids).await?;
    let sessions = coding_session::list_for_studio_items(&mut connection, &item_ids).await?;
    drop(connection);

    Ok(Json(json!({
        "items": StudioItemResponse::many(items, &assets),
        "sessions": session_responses(&state, sessions),
    })))
}
