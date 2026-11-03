// Copyright © 2026 Jalapeno Labs

//! `/api/v1/accounts`: every account the signed-in person sees, with the connectors each feeds.
//! See `crate::connectors::accounts` and `docs/connectors.md`.
//!
//! Each kind of account is still added and removed where it lives: Atlassian and GitHub
//! accounts with their connector, or under `/jira-credentials` and `/github-credentials`;
//! browsers under `/browser-sessions`. An API key's account is removed here, which removes the
//! connectors it feeds.

use anyhow::Context;
use axum::Json;
use axum::Router;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::connectors::accounts;
use crate::errors::ApiError;
use crate::models::connector::{self, AccountRef};
use crate::models::token_account;
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_accounts))
        .route("/tokens/{id}", delete(delete_token_account))
}

/// `GET /api/v1/accounts`.
async fn list_accounts(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let accounts = accounts::list(&mut connection, current.id()).await?;
    Ok(Json(json!({ "accounts": accounts })))
}

/// `DELETE /api/v1/accounts/tokens/{id}`: forget one of the person's API keys, and stop every
/// connector it feeds. The key itself stays valid with its provider until revoked there.
async fn delete_token_account(
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
    let account = token_account::find_owned(&mut connection, id, current.id()).await?;
    let fed = connector::list_by_account(&mut connection, AccountRef::Token(account.id)).await?;
    token_account::delete(&mut connection, account.id).await?;
    for removed in fed {
        state
            .events
            .publish(&ServerEvent::ConnectorDeleted { id: removed.id });
    }
    Ok(StatusCode::NO_CONTENT)
}
