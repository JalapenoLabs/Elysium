// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/knowledge/asks`: asks the knowledge base a question, `{ question, sources,
//! project, person, from, to }`, and answers `201` with the ask, queued for the engine or, when
//! the search found nothing, already answered with nothing to say.
//!
//! The work is [`ask`], which the MCP server's `knowledge_ask` calls too, with
//! [`AskKnowledgeRequest`] as its arguments.

use anyhow::Context as _;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use diesel_async::AsyncConnection as _;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use super::KnowledgeAskResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::knowledge::ask::{self as asking, ASK_DOCUMENTS, ASK_WINDOW};
use crate::knowledge::search::MAX_QUERY_CHARS;
use crate::models::knowledge_ask::{self, KnowledgeAskState, NewKnowledgeAsk, PassageReference};
use crate::realtime::{ServerEvent, Signal};
use crate::routes::v1::knowledge::{SearchKnowledgeRequest, search};
use crate::state::AppState;

/// A question for the knowledge base. Every filter narrows what is searched for the answer, as
/// the search's do; none hides anything from a member.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AskKnowledgeRequest {
    /// The question, in a sentence or two, up to 500 characters, as a person would ask a
    /// coworker: "What did we decide about billing retries?" It is also what is searched for,
    /// so name the people, projects, keys, and terms it is about.
    #[validate(
        length(max = MAX_QUERY_CHARS),
        custom(function = "validate_question")
    )]
    pub question: String,
    /// Only documents of these sources, such as `slack`, `outlook`, `teams`, `confluence`,
    /// `youtrack`, `meeting` (a meeting's topics), `learning`, or `company_doc`. Every source
    /// when left out.
    #[serde(default)]
    pub sources: Vec<String>,
    /// Only what was attributed to this project, by its id.
    pub project: Option<Uuid>,
    /// Only what came from this person, by their user id.
    pub person: Option<Uuid>,
    /// Only what was said at or after this moment, with an offset, such as
    /// `2026-11-04T09:00:00Z`.
    pub from: Option<DateTime<Utc>>,
    /// Only what was said before this moment, with an offset.
    pub to: Option<DateTime<Utc>>,
}

/// A question with nothing in it asks nothing.
fn validate_question(question: &str) -> Result<(), ValidationError> {
    if question.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("ask a question".into()));
    }
    Ok(())
}

impl AskKnowledgeRequest {
    /// The filters as the ask keeps them: only those asked for.
    fn filters(&self) -> Value {
        let mut filters = Map::new();
        if !self.sources.is_empty() {
            filters.insert("sources".to_owned(), json!(self.sources));
        }
        let optional = [
            ("project", self.project.map(|project| json!(project))),
            ("person", self.person.map(|person| json!(person))),
            ("from", self.from.map(|from| json!(from))),
            ("to", self.to.map(|to| json!(to))),
        ];
        for (name, value) in optional {
            if let Some(value) = value {
                filters.insert(name.to_owned(), value);
            }
        }
        Value::Object(filters)
    }
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    body: Result<Json<AskKnowledgeRequest>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(request) = body?;
    let stored = ask(&state, &member, request).await?;
    Ok((StatusCode::CREATED, Json(json!({ "ask": stored }))))
}

/// Asks the knowledge base `request`'s question for `asker`: searches as them, spending one of
/// their searches, then records the ask with what the search offered and signals the engine.
///
/// # Errors
/// [`ApiError::Validation`] for a question or filters outside their limits;
/// [`ApiError::TooManyRequests`] with the code `knowledge_ask_rate_limited` once `asker` has
/// asked [`crate::knowledge::ask::ASKS_PER_WINDOW`] questions in the last hour or has
/// [`crate::knowledge::ask::WAITING_PER_PERSON`] still waiting, or with the search's own code
/// once their searches are spent; and an internal error when the database does not answer.
pub async fn ask(
    state: &AppState,
    asker: &MemberUser,
    request: AskKnowledgeRequest,
) -> Result<KnowledgeAskResponse, ApiError> {
    request.validate()?;
    let filters = request.filters();
    let question = request.question.trim().to_owned();

    // The search validates the filters and spends the asker's search budget, as any search does.
    let found = search(
        &state.knowledge,
        &state.database,
        asker.id(),
        SearchKnowledgeRequest {
            query: question.clone(),
            sources: request.sources,
            project: request.project,
            person: request.person,
            from: request.from,
            to: request.to,
            limit: Some(ASK_DOCUMENTS),
        },
    )
    .await?;
    let offered: Vec<PassageReference> = found
        .results
        .iter()
        .flat_map(|result| {
            result.passages.iter().map(|passage| PassageReference {
                document_id: result.citation.document_id,
                position: passage.position,
            })
        })
        .collect();
    let new_ask = NewKnowledgeAsk {
        id: Uuid::now_v7(),
        created_by: asker.id(),
        question,
        filters,
        offered,
        searched_by_meaning: found.searched_by_meaning,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    // Counted and recorded under the asker's lock, so asks made at once cannot all pass a count
    // that none of them is in yet.
    let recorded = connection
        .transaction(async move |connection| {
            knowledge_ask::lock_asks_of(connection, new_ask.created_by).await?;
            let now = Utc::now();
            let spent =
                knowledge_ask::spent_by(connection, new_ask.created_by, now - ASK_WINDOW).await?;
            if let Some(refusal) = asking::budget_refusal(&spent, now) {
                return Ok(Err(refusal));
            }
            knowledge_ask::insert(connection, new_ask).await.map(Ok)
        })
        .await
        .map_err(ApiError::from)?;
    let stored = recorded.map_err(|message| ApiError::TooManyRequests {
        code: "knowledge_ask_rate_limited",
        message,
    })?;

    if stored.state == KnowledgeAskState::Queued {
        state.events.signal(&Signal::KnowledgeAsks);
    }
    state.events.publish(&ServerEvent::KnowledgeAskChanged {
        id: stored.id,
        asked_by: stored.created_by,
        state: stored.state,
    });
    Ok(stored.into())
}
