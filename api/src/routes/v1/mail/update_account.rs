// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/mail/accounts/{id}`: rename the sender or (de)activate a mailbox.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::MailAccountResponse;
use crate::errors::ApiError;
use crate::models::mail_account::{self, MailAccountChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(max = 120))]
    display_name: Option<String>,
    is_active: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let changes = MailAccountChanges {
        display_name: body.display_name.map(|name| name.trim().to_owned()),
        is_active: body.is_active,
    };
    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let account = mail_account::update(&mut connection, id, &changes).await?;

    state
        .events
        .publish(&ServerEvent::MailAccountUpserted(account.clone().into()));
    Ok(Json(
        json!({ "account": MailAccountResponse::from(account) }),
    ))
}
