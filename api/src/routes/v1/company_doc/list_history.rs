// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/company-doc/history`: every change to the doc, oldest first: who made it, the
//! proposal it approved when it was one, and each section's text before and after.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::auth::MemberUser;
use crate::company_doc::HistoryEntryResponse;
use crate::errors::ApiError;
use crate::models::company_doc;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let history: Vec<HistoryEntryResponse> = company_doc::history(&mut connection)
        .await?
        .into_iter()
        .map(HistoryEntryResponse::from)
        .collect();

    Ok(Json(json!({ "history": history })))
}
