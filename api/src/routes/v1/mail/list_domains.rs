// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/domains`: every domain the mail server hosts, alphabetically.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::MailDomainResponse;
use crate::errors::ApiError;
use crate::models::mail_domain;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let domains: Vec<MailDomainResponse> = mail_domain::list(&mut connection)
        .await?
        .into_iter()
        .map(MailDomainResponse::from)
        .collect();
    Ok(Json(json!({ "domains": domains })))
}
