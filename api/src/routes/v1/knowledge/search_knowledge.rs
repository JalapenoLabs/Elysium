// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/knowledge/search?q=&source=&project=&person=&label=&from=&to=&limit=`: the documents
//! whose passages match `q` by their words or their meaning, best first, each with its best
//! passages and its citation. See `crate::knowledge::search` and `docs/api.md`, Knowledge.
//!
//! - `source` is a comma-separated list of the index's source names, such as `slack,learning`;
//! - `project` a project's id, which keeps timeline records attributed to it;
//! - `person` a user's id, which keeps what came from them, as a narrowing and never a limit on
//!   what the caller may read;
//! - `label` a page label, which keeps the pages carrying it;
//! - `from` and `to` moments with an offset, what was said from one and before the other.
//!
//! The work is [`search`], which the MCP server's `knowledge_search` and the agents' tool of the
//! same name call too, with [`SearchKnowledgeRequest`] as their arguments: the route's query is
//! only its form in a URL.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::knowledge::search::{self, KnowledgeSearch, SearchAnswer, SearchFilters, SearchRequest};
use crate::middleware::rate_limit::{Decision, whole_seconds};
use crate::models::page_label::Label;
use crate::routes::v1::action_items::comma_separated;
use crate::state::AppState;

/// A search of the knowledge base. Every filter narrows what is searched; none hides anything
/// from a member, since every member may read everything indexed.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[validate(schema(function = "validate_period"))]
pub struct SearchKnowledgeRequest {
    /// What to look for, up to 500 characters: a few words, a question in a sentence, or an
    /// exact key, name, or error message. Words are matched as a search box matches them:
    /// "quotes" for a phrase, `or` for either, a leading `-` for without.
    #[validate(length(max = search::MAX_QUERY_CHARS))]
    pub query: String,
    /// Only documents of these sources, such as `slack`, `outlook`, `teams`, `confluence`,
    /// `youtrack`, `meeting` (a meeting's topics), `learning`, or `company_doc`. Every source
    /// when left out.
    #[serde(default)]
    #[validate(
        length(max = search::MAX_SOURCES),
        custom(function = "validate_sources")
    )]
    pub sources: Vec<String>,
    /// Only what was attributed to this project, by its id. Learnings and the company doc
    /// belong to no project, so this leaves them out.
    pub project: Option<Uuid>,
    /// Only what came from this person, by their user id: their mail, messages, meetings,
    /// learnings, and company doc edits.
    pub person: Option<Uuid>,
    /// Only pages carrying this label, however it is cased or spaced. Nothing but pages has
    /// labels, so this leaves everything else out. A learning's category is not a label.
    pub label: Option<String>,
    /// Only what was said at or after this moment, with an offset, such as
    /// `2026-11-04T09:00:00Z`.
    pub from: Option<DateTime<Utc>>,
    /// Only what was said before this moment, with an offset.
    pub to: Option<DateTime<Utc>>,
    /// How many documents to answer, from 1 to 50. Defaults to 10.
    #[validate(range(min = 1, max = search::MAX_LIMIT))]
    pub limit: Option<usize>,
}

/// The route's query: [`SearchKnowledgeRequest`] as a URL carries it, `q` for the query and the
/// sources comma-separated.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SearchQuery {
    q: String,
    #[serde(default, deserialize_with = "comma_separated")]
    source: Vec<String>,
    project: Option<Uuid>,
    person: Option<Uuid>,
    label: Option<String>,
    from: Option<DateTime<Utc>>,
    to: Option<DateTime<Utc>>,
    limit: Option<usize>,
}

impl From<SearchQuery> for SearchKnowledgeRequest {
    fn from(query: SearchQuery) -> Self {
        Self {
            query: query.q,
            sources: query.source,
            project: query.project,
            person: query.person,
            label: query.label,
            from: query.from,
            to: query.to,
            limit: query.limit,
        }
    }
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
fn validate_period(request: &SearchKnowledgeRequest) -> Result<(), ValidationError> {
    match (request.from, request.to) {
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
    let Query(query) = query?;
    let answer = search(&state.knowledge, &state.database, member.id(), query.into()).await?;
    Ok(Json(json!(answer)))
}

/// Searches the knowledge base for `reader`, spending one of their searches first.
///
/// `reader` is whoever the search is for: a member, or Elysia for her own turns. Callers check
/// that they may read the knowledge base before asking (`docs/knowledge.md`, Who sees what);
/// nothing here limits what is found by whose record it is.
///
/// # Errors
/// [`ApiError::TooManyRequests`] once `reader` has spent their budget, which a request refused
/// for its arguments still spends; [`ApiError::Validation`] for arguments outside their limits,
/// or a label no page could carry; and an internal error when the database does not answer.
pub async fn search(
    knowledge: &KnowledgeSearch,
    database: &Pool,
    reader: Uuid,
    request: SearchKnowledgeRequest,
) -> Result<SearchAnswer, ApiError> {
    // Spent before the arguments are checked, so a search refused for them still counts.
    if let Decision::Refused { wait } = knowledge.admit(reader) {
        let seconds = whole_seconds(wait);
        return Err(ApiError::TooManyRequests {
            code: "knowledge_search_rate_limited",
            message: format!("Too many knowledge searches; try again in {seconds} seconds"),
        });
    }
    request.validate()?;
    // Read as a label is written, so a filter typed `On Call` finds the label `on-call`.
    let label = request.label.as_deref().map(Label::parse).transpose()?;

    let request = SearchRequest {
        query: request.query,
        filters: SearchFilters {
            sources: request.sources,
            project_id: request.project,
            person_id: request.person,
            occurred_from: request.from,
            occurred_before: request.to,
            label: label.map(|label| label.as_str().to_owned()),
        },
        limit: request.limit.unwrap_or(search::DEFAULT_LIMIT),
    };
    Ok(knowledge.search(database, &request).await?)
}
