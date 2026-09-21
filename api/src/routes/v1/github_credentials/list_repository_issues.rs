// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/github-credentials/{id}/repositories/{owner}/{name}/issues`: a repository's
//! open issues, or its open pull requests with `kind=pull-request`, most recently updated
//! first, for picking one to link to an action item.
//!
//! One page of a hundred is read, which covers what a person picks from; `truncated` says
//! when the repository has more.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::repository_of;
use crate::errors::ApiError;
use crate::github::issues::IssueQuery;
use crate::models::action_item_link::LinkKind;
use crate::models::github_credential;
use crate::state::AppState;

/// How many pages of open issues a picker reads: one, a hundred of the most recently
/// updated.
const PICKER_PAGE_LIMIT: usize = 1;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryParameters {
    /// `issue` (the default) or `pull-request`.
    #[serde(default = "issues")]
    kind: LinkKind,
}

const fn issues() -> LinkKind {
    LinkKind::Issue
}

/// One open issue or pull request as a picker lists it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Choice {
    /// `owner/name#12`, which a link request names it by.
    reference: String,
    number: u64,
    title: String,
    url: String,
    /// The login of the first assignee.
    assignee: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<(Uuid, String, String)>, PathRejection>,
    query: Result<Query<QueryParameters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, owner, name)) = path?;
    let Query(query) = query?;
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

    let open_only = IssueQuery {
        open_only: true,
        ..IssueQuery::default()
    };
    let listing = state
        .github
        .list_issues(&token, &repository, &open_only, PICKER_PAGE_LIMIT)
        .await?;
    let wants_pull_requests = query.kind == LinkKind::PullRequest;
    let choices: Vec<Choice> = listing
        .items
        .into_iter()
        .filter(|issue| issue.pull_request.is_some() == wants_pull_requests)
        .map(|issue| Choice {
            reference: issue.reference(),
            number: issue.number,
            title: issue.title,
            url: issue.url,
            assignee: issue.assignee,
        })
        .collect();

    Ok(Json(json!({
        "issues": choices,
        "truncated": listing.truncated,
    })))
}
