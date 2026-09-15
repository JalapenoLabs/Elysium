// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/accounts`: every connected mailbox.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::MailAccountResponse;
use crate::errors::ApiError;
use crate::models::mail_account;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let accounts: Vec<MailAccountResponse> = mail_account::list(&mut connection)
        .await?
        .into_iter()
        .map(MailAccountResponse::from)
        .collect();

    Ok(Json(json!({ "accounts": accounts })))
}
