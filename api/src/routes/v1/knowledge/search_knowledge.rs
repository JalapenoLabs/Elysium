// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/knowledge/search?q=&source=&project=&person=&from=&to=&limit=`: the documents
//! whose passages match `q` by their words or their meaning, best first, each with its best
//! passages and its citation. See `crate::knowledge::search` and `docs/api.md`, Knowledge.
//!
//! - `source` is a comma-separated list of the index's source names, such as `slack,learning`;
//! - `project` a project's id, which keeps timeline records attributed to it;
//! - `person` a user's id, which keeps what came from them, as a narrowing and never a limit on
//!   what the caller may read;
//! - `from` and `to` moments with an offset, what was said from one and before the other.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::knowledge::search::{self, SearchFilters, SearchRequest};
use crate::middleware::rate_limit::{Decision, whole_seconds};
use crate::routes::v1::action_items::comma_separated;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[validate(schema(function = "validate_period"))]
pub struct SearchQuery {
    #[validate(length(max = search::MAX_QUERY_CHARS))]
    q: String,
    #[serde(default, deserialize_with = "comma_separated")]
    #[validate(
        length(max = search::MAX_SOURCES),
        custom(function = "validate_sources")
    )]
    source: Vec<String>,
    project: Option<Uuid>,
    person: Option<Uuid>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    #[validate(range(min = 1, max = search::MAX_LIMIT))]
    limit: Option<usize>,
}

/// A source is named as `knowledge_documents.source` names it: a lowercase letter, then up to
/// 31 lowercase letters, digits, or underscores.
fn validate_sources(sources: &[String]) -> Result<(), ValidationError> {
    let is_source_name = |source: &String| {
        let mut characters = source.chars();
        let starts_with_a_letter = characters
            .next()
            .is_some_and(|first| first.is_ascii_lowercase());
        let rest_is_a_name = characters.all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
        });
        starts_with_a_letter && rest_is_a_name && source.len() <= 32
    };
    if sources.iter().all(is_source_name) {
        return Ok(());
    }
    Err(ValidationError::new("source").with_message(
        "a source is a lowercase letter, then letters, digits, or underscores".into(),
    ))
}

/// A period that ends where it starts, or before, holds nothing, which is a mistake to say
/// rather than answer as no results.
fn validate_period(query: &SearchQuery) -> Result<(), ValidationError> {
    match (query.from, query.to) {
        (Some(from), Some(to)) if from >= to => {
            Err(ValidationError::new("period").with_message("`from` must be before `to`".into()))
        }
        _ => Ok(()),
    }
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    // Spent before anything else, so a request refused for its query still counts.
    if let Decision::Refused { wait } = state.knowledge.admit(member.id()) {
        let seconds = whole_seconds(wait);
        return Err(ApiError::TooManyRequests {
            code: "knowledge_search_rate_limited",
            message: format!("Too many knowledge searches; try again in {seconds} seconds"),
        });
    }

    let Query(query) = query?;
    query.validate()?;

    let request = SearchRequest {
        query: query.q,
        filters: SearchFilters {
            sources: query.source,
            project_id: query.project,
            person_id: query.person,
            occurred_from: query.from,
            occurred_before: query.to,
        },
        limit: query.limit.unwrap_or(search::DEFAULT_LIMIT),
    };
    let answer = state.knowledge.search(&state.database, &request).await?;

    Ok(Json(json!(answer)))
}
