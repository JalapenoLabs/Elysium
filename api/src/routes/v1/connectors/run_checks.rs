// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors/{id}/checks`: run a stored connector's checks now. Answers `202` with
//! the run at once; its progress arrives as `connectorCheckRun.updated`, and each check's result
//! replaces the connector's last when the run ends.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

use super::visible;
use crate::auth::CurrentUser;
use crate::connectors::kinds::{self, Source};
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let stored = visible(&state, id, current.id()).await?;
    let kind = kinds::kind_of(&stored)?;
    let checks = kinds::prepare_checks(&state, &current, kind, Source::Stored(&stored)).await?;
    let run = kinds::start_checks(&state, &current, kind, Some(stored.id), checks);
    Ok((StatusCode::ACCEPTED, Json(json!({ "run": run }))))
}
