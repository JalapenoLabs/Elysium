// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/boards`: the boards the stored token can reach now,
//! marked with what the credential already allows, for the edit form.

use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::open;
use crate::errors::ApiError;
use crate::models::jira_credential::Allowlist;
use crate::state::AppState;

/// One board a token can reach, and whether this credential already allows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoardChoice {
    id: i64,
    name: String,
    project_key: Option<String>,
    selected: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let stored = open(&state, id).await?;
    let listing = state.jira.list_boards(&stored.site()).await?;

    let boards: Vec<BoardChoice> = listing
        .items
        .into_iter()
        .map(|board| BoardChoice {
            selected: match &stored.allowed.boards {
                Allowlist::All => true,
                Allowlist::Only(allowed) => allowed.iter().any(|picked| picked.id == board.id),
            },
            id: board.id,
            name: board.name,
            project_key: board.project_key,
        })
        .collect();

    Ok(Json(json!({
        "boards": boards,
        "truncated": listing.truncated,
    })))
}
