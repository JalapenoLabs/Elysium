// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/mail/accounts/{id}/test`: sign in to the mailbox's servers right now.
//!
//! The outcome is recorded on the account either way, so a failure answers with the
//! updated account rather than an error status: the check itself worked, the mailbox
//! did not. Only a fault inside Elysium, such as a credential that no longer decrypts,
//! is an error response.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MailAccountResponse, open_mailbox};
use crate::errors::ApiError;
use crate::mail::transport;
use crate::models::mail_account;
use crate::realtime::ServerEvent;
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
    let account = mail_account::find(&mut connection, id).await?;
    drop(connection);

    let outcome = match open_mailbox(&state, &account).await {
        Ok(mailbox) => transport::check(&mailbox).await.map_err(ApiError::from),
        Err(error) => Err(error),
    };
    let failure = match outcome {
        Ok(()) => None,
        Err(ApiError::BadGateway(message)) => Some(message),
        Err(other) => return Err(other),
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let checked = mail_account::record_check(&mut connection, id, failure.as_deref()).await?;

    state
        .events
        .publish(&ServerEvent::MailAccountUpserted(checked.clone().into()));
    Ok(Json(
        json!({ "account": MailAccountResponse::from(checked) }),
    ))
}
