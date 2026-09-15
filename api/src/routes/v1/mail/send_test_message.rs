// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/mail/accounts/{id}/test-message`: send a short message from the
//! mailbox to itself, proving the send path end to end.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::open_mailbox;
use crate::errors::ApiError;
use crate::mail::transport;
use crate::models::mail_account;
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

    let mailbox = open_mailbox(&state, &account).await?;
    let body = format!(
        "This is a test message from Elysium, sent at {} UTC.\n\nIf it arrived, this mailbox can send mail.\n",
        Utc::now().format("%Y-%m-%d %H:%M:%S"),
    );
    transport::send_text(&mailbox, &account.address, "Elysium test message", body).await?;

    Ok(Json(json!({ "sentTo": account.address })))
}
