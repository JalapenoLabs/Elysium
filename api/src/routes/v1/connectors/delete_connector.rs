// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/connectors/{id}`: remove a connector and the account it owns, which signs it
//! out: its token is forgotten, or its browser is removed with the profile its person signed in
//! on. What it recorded stays on the timeline.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::visible;
use crate::auth::CurrentUser;
use crate::connectors::kinds;
use crate::errors::ApiError;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let stored = visible(&state, id, current.id()).await?;
    kinds::remove(&state, &stored).await?;
    state.events.publish(&ServerEvent::ConnectorDeleted { id });
    Ok(StatusCode::NO_CONTENT)
}
