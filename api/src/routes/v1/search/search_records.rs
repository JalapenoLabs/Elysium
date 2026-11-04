// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/search?q=&limit=`: the action items, projects, coding sessions, and
//! initiatives whose titles match `q`, the closest `limit` of each kind (5 unless asked, at
//! most 50), each kind in its own order.
//!
//! Each kind is searched among exactly the rows its own list route shows its caller: items and
//! initiatives not deleted, every project, and the Coding area's sessions. Those lists answer
//! everyone with access to the workspace, guests included (`docs/auth.md`, Roles), so this
//! does too. A result carries what a row of results shows and no more: its kind, id, title,
//! and, for items and initiatives, their state.
//!
//! The knowledge base joins as a kind of its own once a search reads it, answering admins
//! and members alone, as its own routes do (`docs/knowledge.md`, Who sees what).

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;
use validator::Validate;

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::middleware::rate_limit::{Decision, whole_seconds};
use crate::models::action_item::{self, ActionItemState};
use crate::models::coding_session;
use crate::models::initiative::{self, InitiativeState};
use crate::models::project;
use crate::search::{self, SearchTerms};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchQuery {
    #[validate(length(max = search::MAX_QUERY_CHARS))]
    q: String,
    #[validate(range(min = 1, max = search::MAX_LIMIT))]
    limit: Option<i64>,
}

/// One record found, as a row of results shows it.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SearchHit {
    ActionItem {
        id: Uuid,
        title: String,
        state: ActionItemState,
    },
    Project {
        id: Uuid,
        title: String,
    },
    CodingSession {
        id: i64,
        title: String,
    },
    Initiative {
        id: Uuid,
        title: String,
        state: InitiativeState,
    },
}

pub async fn handle(
    State(state): State<AppState>,
    user: CurrentUser,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    // Spent before anything else, so a request refused for its query still counts.
    if let Decision::Refused { wait } = state.search_limiter.decide(&user.id()) {
        let seconds = whole_seconds(wait);
        return Err(ApiError::TooManyRequests {
            code: "search_rate_limited",
            message: format!("Too many searches; try again in {seconds} seconds"),
        });
    }

    let Query(query) = query?;
    query.validate()?;

    let terms = SearchTerms::parse(&query.q);
    if terms.is_empty() {
        // Only spaces or punctuation: no word to match, as the web app's own search finds
        // nothing for one.
        event!(
            name: "search.query.empty",
            Level::DEBUG,
            "a search held no words, so it matches nothing",
        );
        return Ok(Json(json!({ "results": [] })));
    }
    let limit = query.limit.unwrap_or(search::DEFAULT_LIMIT);

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let mut results = Vec::new();
    for (id, title, state) in action_item::search(&mut connection, &terms, limit).await? {
        results.push(SearchHit::ActionItem { id, title, state });
    }
    for (id, title) in project::search(&mut connection, &terms, limit).await? {
        results.push(SearchHit::Project { id, title });
    }
    for (id, title) in coding_session::search(&mut connection, &terms, limit).await? {
        results.push(SearchHit::CodingSession { id, title });
    }
    for (id, title, state) in initiative::search(&mut connection, &terms, limit).await? {
        results.push(SearchHit::Initiative { id, title, state });
    }

    Ok(Json(json!({ "results": results })))
}
