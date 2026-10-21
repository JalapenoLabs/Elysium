// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/connectors/{id}`: stop reading a service. What it recorded stays on the
//! timeline, and the browser stays signed in.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::connector_account;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let account = connector_account::find_owned(&mut connection, id, current.id()).await?;
    connector_account::delete(&mut connection, account.id).await?;

    state
        .events
        .publish(&ServerEvent::ConnectorAccountDeleted { id: account.id });
    Ok(StatusCode::NO_CONTENT)
}
