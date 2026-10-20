// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/studio-items/{id}`: one item, live or softly deleted, with every file its
//! agents produced (newest first), every drawn prompt (oldest first), and its sessions in the
//! order its conversation reads.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{StudioAssetResponse, StudioFeedbackResponse, StudioItemResponse, session_responses};
use crate::errors::ApiError;
use crate::models::{coding_session, studio_asset, studio_feedback, studio_item};
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
    let item = studio_item::find(&mut connection, id).await?;
    let assets = studio_asset::list_for_items(&mut connection, &[id]).await?;
    let feedback = studio_feedback::list_for_item(&mut connection, id).await?;
    let sessions = coding_session::list_for_studio_items(&mut connection, &[id]).await?;
    drop(connection);

    let asset_references: Vec<_> = assets.iter().collect();
    let item = StudioItemResponse::new(item, &asset_references);
    let assets: Vec<StudioAssetResponse> = assets.into_iter().map(Into::into).collect();
    let feedback: Vec<StudioFeedbackResponse> = feedback.into_iter().map(Into::into).collect();

    Ok(Json(json!({
        "item": item,
        "assets": assets,
        "feedback": feedback,
        "sessions": session_responses(&state, sessions),
    })))
}
