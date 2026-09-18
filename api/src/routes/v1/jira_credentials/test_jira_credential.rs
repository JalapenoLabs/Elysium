// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/jira-credentials/{id}/test`: ask Jira about the stored token right now,
//! record what it answers, and say which of the stored picks the token can still reach.
//!
//! A project the token has lost access to is reported, not dropped: the credential keeps
//! promising it until someone changes the selection, and the settings page can say so.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{JiraCredentialResponse, open, reach};
use crate::errors::ApiError;
use crate::models::jira_credential::{self, Allowlist};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// One stored project, and whether the token still reaches it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectCheck {
    id: String,
    key: String,
    name: String,
    reachable: bool,
}

/// One stored board, and whether the token still reaches it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct BoardCheck {
    id: i64,
    name: String,
    project_key: Option<String>,
    reachable: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let stored = open(&state, id).await?;

    // A token can be revoked or rotated on Atlassian, and a project's permissions can change
    // under it, so what Jira answers now replaces what the row remembered.
    let reachable = reach(&state.jira, &stored.site()).await?;

    let projects: Vec<ProjectCheck> = match &stored.allowed.projects {
        Allowlist::All => Vec::new(),
        Allowlist::Only(allowed) => allowed
            .iter()
            .map(|project| ProjectCheck {
                reachable: reachable
                    .projects
                    .iter()
                    .any(|reported| reported.id == project.id),
                id: project.id.clone(),
                key: project.key.clone(),
                name: project.name.clone(),
            })
            .collect(),
    };
    let boards: Vec<BoardCheck> = match &stored.allowed.boards {
        Allowlist::All => Vec::new(),
        Allowlist::Only(allowed) => allowed
            .iter()
            .map(|board| BoardCheck {
                reachable: reachable
                    .boards
                    .iter()
                    .any(|reported| reported.id == board.id),
                id: board.id,
                name: board.name.clone(),
                project_key: board.project_key.clone(),
            })
            .collect(),
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = jira_credential::record_check(&mut connection, id, &reachable.account).await?;
    drop(connection);

    let allowed = stored.allowed;
    let response = JiraCredentialResponse::new(credential.clone(), allowed.clone());
    state
        .events
        .publish(&ServerEvent::JiraCredentialUpserted(response));

    Ok(Json(json!({
        "credential": JiraCredentialResponse::new(credential, allowed),
        "result": {
            "account": reachable.account,
            "projects": projects,
            "boards": boards,
        },
    })))
}
