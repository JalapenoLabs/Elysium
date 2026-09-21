// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/github-credentials/{id}/repositories/{owner}/{name}/labels`: a repository's
//! labels, for linking one to an initiative as a container.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{CONTAINER_PAGE_LIMIT, repository_of};
use crate::errors::ApiError;
use crate::models::github_credential;
use crate::state::AppState;

/// One label as a picker lists it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Choice {
    /// `owner/name:label`, which a link request names it by.
    reference: String,
    name: String,
    url: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String, String)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, owner, name)) = path?;
    let repository = repository_of(&owner, &name)?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = github_credential::find(&mut connection, id).await?;
    drop(connection);
    let token = credential
        .token(&state.cipher)
        .context("the stored token cannot be decrypted")?;

    let listing = state
        .github
        .list_labels(&token, &repository, CONTAINER_PAGE_LIMIT)
        .await?;
    let choices: Vec<Choice> = listing
        .items
        .into_iter()
        .map(|label| Choice {
            reference: format!("{}/{}:{}", repository.owner, repository.name, label.name),
            name: label.name,
            url: label.url,
        })
        .collect();

    Ok(Json(json!({
        "labels": choices,
        "truncated": listing.truncated,
    })))
}
