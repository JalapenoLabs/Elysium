// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/connectors/{id}`: stop reading through an account. What it recorded stays on
//! the timeline. A Jira or GitHub connector goes with its account, which carries exactly one;
//! any other leaves its account, and the account's sign-in, as they were.

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
