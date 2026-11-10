// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/findings/{id}/dismiss`: a person dismisses an open finding, with an optional
//! `reason` the agents that report it again are told. The finding stays, so it is never raised
//! again. Answers the finding. `409` (`finding_decided`) once someone decided it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::FindingResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::findings;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    reason: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    person: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let finding = findings::dismiss(
        &mut connection,
        id,
        &person,
        body.reason.as_deref(),
        Utc::now(),
    )
    .await?;
    findings::publish(&state.events, &finding);
    Ok(Json(json!({ "finding": FindingResponse::from(finding) })))
}
