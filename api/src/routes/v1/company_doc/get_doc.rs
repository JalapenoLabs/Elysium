// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/company-doc`: every section in order, each with its pending proposals. The MCP
//! server's `company_doc_read` and agents' tool of the same name answer the same.

use anyhow::Context;
use axum::Json;
use axum::extract::State;

use crate::auth::MemberUser;
use crate::company_doc::{self, CompanyDocResponse};
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
) -> Result<Json<CompanyDocResponse>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    Ok(Json(company_doc::read(&mut connection).await?))
}
