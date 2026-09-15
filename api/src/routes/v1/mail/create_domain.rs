// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/mail/domains`: host another domain on the mail server.
//!
//! The domain shares the server's listeners and hostname with every other domain.
//! Stalwart generates its DKIM keys in the background, within seconds.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{MailDomainResponse, require_administrator, validate_domain};
use crate::errors::ApiError;
use crate::models::mail_domain;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 3, max = 253), custom(function = "validate_domain"))]
    name: String,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;
    let name = body.name.to_lowercase();

    let administrator = require_administrator(
        &state,
        "a mail server must be created before domains can be added",
    )
    .await?;

    // Finds the domain when Stalwart already has it, which is how the server's first
    // domain is recorded if creating the server stopped before recording it. It is then
    // recorded without the default flag, and Stalwart itself still refuses to remove it.
    let stalwart_id = state
        .mail
        .stalwart
        .ensure_domain(&administrator, &name)
        .await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let created = mail_domain::create(&mut connection, &name, &stalwart_id, false).await?;

    let response = MailDomainResponse::from(created);
    let body = json!({ "domain": &response });
    state
        .events
        .publish(&ServerEvent::MailDomainUpserted(response));
    Ok((StatusCode::CREATED, Json(body)))
}
