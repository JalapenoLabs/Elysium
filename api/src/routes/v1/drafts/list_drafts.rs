// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/drafts?recordId=<id>`: the signed-in person's drafts answering one record that
//! were never sent, newest first, which the record's reply box shows.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::DraftResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::draft;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Filters {
    record_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<Filters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(filters) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let drafts =
        draft::list_unsent_for_record(&mut connection, filters.record_id, current.id()).await?;
    let drafts: Vec<DraftResponse> = drafts.into_iter().map(DraftResponse::from).collect();
    Ok(Json(json!({ "drafts": drafts })))
}
