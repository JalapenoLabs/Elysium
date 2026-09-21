// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/initiatives/{id}/links`: the containers an initiative is linked to, oldest
//! first, with how the watcher's latest read of each went.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::InitiativeLinkResponse;
use crate::errors::ApiError;
use crate::models::{initiative, initiative_link};
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
    initiative::find(&mut connection, id).await?;
    let links: Vec<InitiativeLinkResponse> =
        initiative_link::list_for_initiative(&mut connection, id)
            .await?
            .into_iter()
            .map(InitiativeLinkResponse::from)
            .collect();

    Ok(Json(json!({ "links": links })))
}
