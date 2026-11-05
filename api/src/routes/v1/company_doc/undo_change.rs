// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/company-doc/history/{entryId}/undo`: reverse one change to the doc.
//!
//! The reversal is an edit of its own, recorded in the history and naming the entry it undid, so
//! nothing is lost and an undo can itself be undone. A change is undone section by section,
//! keeping any section changed again since; see `crate::models::company_doc::undo`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use uuid::Uuid;

use super::author;
use crate::auth::MemberUser;
use crate::company_doc::{self, CompanyDocResponse};
use crate::errors::ApiError;
use crate::models::company_doc as doc;
use crate::routes::v1::pages;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<CompanyDocResponse>, ApiError> {
    let Path(entry_id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let entry = doc::undo(&mut connection, entry_id, author(&member)).await?;
    if let Some(entry) = entry {
        pages::announce(&state.events, &entry.pages(), &[]);
    }

    Ok(Json(company_doc::read(&mut connection).await?))
}
