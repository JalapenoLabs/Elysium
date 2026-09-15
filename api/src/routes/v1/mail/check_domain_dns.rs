// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/domains/{id}/dns`: the records a domain needs, checked against
//! public DNS.
//!
//! Records come from Stalwart, which generates them per domain; see
//! [`crate::mail::dns`]. The full zone file is included for the optional records the
//! check leaves out.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::require_administrator;
use crate::errors::ApiError;
use crate::mail::dns::essential_records;
use crate::models::mail_domain;
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
    let domain = mail_domain::find(&mut connection, id).await?;
    drop(connection);

    let administrator = require_administrator(
        &state,
        "the mail server does not exist, so it has no records for the domain",
    )
    .await?;
    let zone_file = state
        .mail
        .stalwart
        .domain_zone_file(&administrator, &domain.stalwart_id)
        .await?;

    let records = state.mail.dns.check(essential_records(&zone_file)).await;
    Ok(Json(json!({ "records": records, "zoneFile": zone_file })))
}
