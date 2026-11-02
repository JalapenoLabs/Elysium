// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/connectors/{id}`: pause or resume a connector (`isActive`), or change what it
//! reads (`content`, in its kind's own terms). Content is checked the way a new connector's is:
//! Jira's picks against what the token reaches now, a storage location against the workspace's.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{view, visible};
use crate::auth::CurrentUser;
use crate::connectors::kinds;
use crate::errors::ApiError;
use crate::models::connector;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    is_active: Option<bool>,
    content: Option<Value>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    if body.is_active.is_none() && body.content.is_none() {
        return Err(ApiError::BadRequest(
            "name isActive, content, or both".to_owned(),
        ));
    }
    let mut stored = visible(&state, id, current.id()).await?;

    if let Some(content) = body.content {
        stored = kinds::update_content(&state, &stored, content).await?;
    }
    if let Some(is_active) = body.is_active {
        let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
        stored = connector::set_active(&mut connection, id, is_active).await?;
    }
    state.connectors.activity.changed(id);
    Ok(Json(json!({ "connector": view(&state, stored).await? })))
}
