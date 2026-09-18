// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/jira-credentials/{id}/issues`: a JQL search, bounded to what the credential
//! may touch.
//!
//! The caller's JQL is rewritten rather than its results filtered, so Jira itself never
//! looks outside the allowed projects (`allowlist::bounded_jql`). Paging is by token: Jira
//! Cloud's search endpoint has no offset and reports no total.

use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use super::{allowlist, open};
use crate::errors::ApiError;
use crate::jira::Search;
use crate::state::AppState;

/// How many issues one page holds by default, and at most. The ceiling is Elysium's own:
/// a page is rendered as a list, and Jira may answer with fewer than it is asked for.
const DEFAULT_MAX_RESULTS: u32 = 50;
const MAX_RESULTS_CEILING: u32 = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchQuery {
    /// The caller's JQL, which is bounded before it is sent. Empty means every allowed
    /// project.
    jql: Option<String>,
    max_results: Option<u32>,
    /// The `nextPageToken` from the previous page. Absent for the first page.
    next_page_token: Option<String>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Query(query) = query?;

    let max_results = query.max_results.unwrap_or(DEFAULT_MAX_RESULTS);
    if max_results == 0 || max_results > MAX_RESULTS_CEILING {
        return Err(ApiError::BadRequest(format!(
            "maxResults is 1 to {MAX_RESULTS_CEILING}"
        )));
    }

    let stored = open(&state, id).await?;
    let jql = query.jql.as_deref().unwrap_or_default();
    let Some(bounded) = allowlist::bounded_jql(jql, &stored.allowed.projects) else {
        event!(
            name: "jira.search.no_projects",
            Level::DEBUG,
            jira.credential.id = %id,
            "a Jira credential with no projects selected can match nothing, so the search was \
             answered empty without calling Jira",
        );
        return Ok(Json(json!(Search {
            issues: Vec::new(),
            next_page_token: None,
            is_last: true,
        })));
    };

    let found = state
        .jira
        .search(
            &stored.site(),
            &bounded,
            max_results,
            query.next_page_token.as_deref(),
        )
        .await?;

    // The search was bounded, so this only ever refuses an answer Jira should not have
    // given. It costs one comparison per issue and closes the gap if it ever does.
    for issue in &found.issues {
        allowlist::ensure_allowed(
            &stored.allowed.projects,
            &stored.credential.name,
            &issue.project_key,
        )?;
    }

    Ok(Json(json!(found)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_search_takes_jql_a_page_size_and_a_token() {
        let query: SearchQuery = serde_json::from_value(json!({
            "jql": "status = Open",
            "maxResults": 25,
            "nextPageToken": "CAEaAggD",
        }))
        .expect("parses");

        assert_eq!(query.jql.as_deref(), Some("status = Open"));
        assert_eq!(query.max_results, Some(25));
        assert_eq!(query.next_page_token.as_deref(), Some("CAEaAggD"));

        let empty: SearchQuery = serde_json::from_value(json!({})).expect("parses");
        assert!(empty.jql.is_none() && empty.max_results.is_none());

        serde_json::from_value::<SearchQuery>(json!({ "startAt": 50 }))
            .expect_err("paging is by token, and an offset would silently do nothing");
    }
}
