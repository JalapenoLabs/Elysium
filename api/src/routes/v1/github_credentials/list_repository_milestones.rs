// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/github-credentials/{id}/repositories/{owner}/{name}/milestones`: a
//! repository's open milestones, for linking one to an initiative as a container.

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

/// One milestone as a picker lists it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Choice {
    /// `owner/name#3`, which a link request names it by.
    reference: String,
    number: u64,
    title: String,
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
        .list_milestones(&token, &repository, CONTAINER_PAGE_LIMIT)
        .await?;
    let choices: Vec<Choice> = listing
        .items
        .into_iter()
        .map(|milestone| Choice {
            reference: format!(
                "{}/{}#{}",
                repository.owner, repository.name, milestone.number
            ),
            number: milestone.number,
            title: milestone.title,
            url: milestone.url,
        })
        .collect();

    Ok(Json(json!({
        "milestones": choices,
        "truncated": listing.truncated,
    })))
}
