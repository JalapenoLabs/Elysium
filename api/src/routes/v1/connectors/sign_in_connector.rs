// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/connectors/{id}/sign-in`: start a browser for a connector read through one
//! that is waiting for its browser, which its person then signs in on from the connector's
//! page. Answers the connector, reading through the new browser, which comes up `starting`.
//! `409` when it already has one, or when the host has no browser port left.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{view, visible};
use crate::auth::CurrentUser;
use crate::connectors::kinds;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let stored = visible(&state, id, current.id()).await?;
    let updated = kinds::sign_in(&state, &stored).await?;
    Ok(Json(json!({ "connector": view(&state, updated).await? })))
}
