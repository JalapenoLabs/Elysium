// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/company-doc/proposals/{id}/approve`: write a proposed change into its section,
//! as a change by the member approving it. A section that changed since the proposal was made
//! gets both changes, merged; one whose changes overlap answers `409 proposal_overlaps` and the
//! proposal stays pending. Answers the proposal as decided and the doc as it now reads.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::author;
use crate::auth::MemberUser;
use crate::company_doc::{self, ProposalResponse};
use crate::errors::ApiError;
use crate::models::company_doc as doc;
use crate::routes::v1::pages;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let proposal = doc::approve(&mut connection, id, author(&member)).await?;
    let changed: Vec<Uuid> = doc::Section::parse(&proposal.section)
        .map(doc::Section::page_id)
        .into_iter()
        .collect();
    pages::announce(&state.events, &changed, &[]);

    Ok(Json(json!({
        "proposal": ProposalResponse::from(proposal),
        "doc": company_doc::read(&mut connection).await?,
    })))
}
