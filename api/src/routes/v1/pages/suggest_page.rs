// Copyright © 2026 Jalapeno Labs

//! What an agent's `page_suggest` does, for the MCP server and a turn's `elysium_knowledge` alike:
//! suggest an edit to a page for a person to accept (`crate::models::page_suggestion`).
//!
//! There is no route: a person edits a page directly, and reviews what agents suggested on the
//! page. The company doc's `company_doc_propose` comes down to the same model function, naming a
//! section rather than a page (`crate::company_doc::propose`).

use anyhow::Context;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{SuggestionResponse, announce_suggested};
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page;
use crate::models::page_suggestion::{self, Edit, NewSuggestion};
use crate::realtime::EventBus;

/// An edit to suggest to a page.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SuggestPageRequest {
    /// The page to change, from `page_list` or `page_read`.
    pub id: Uuid,
    /// The `currentVersion` `page_read` answered: the version the edits were written against. A
    /// page published since is no reason to read it again; a person accepting it merges the two.
    pub base_version: i32,
    /// The changes, applied in order to that version, as Claude Code's Edit tool takes them:
    /// `find` is text the page holds exactly once, copied exactly, and `replace` takes its place.
    /// An empty `find` adds `replace` at the end of the page. At most 20.
    pub edits: Vec<Edit>,
    /// Why the page should say this, in a sentence or two the person reviewing it reads first.
    pub reason: String,
    /// The words that show it, quoted exactly.
    #[serde(default)]
    pub quote: Option<String>,
    /// Where the quote is from, such as a meeting's title or a channel and its date.
    #[serde(default)]
    pub source: Option<String>,
}

/// A suggestion, and whether suggesting it stored it or found the same edit already pending.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestedPage {
    pub suggestion: SuggestionResponse,
    /// `false` when the same edit was already waiting for review.
    pub created: bool,
    /// What happens next, for the agent that suggested it.
    pub message: &'static str,
}

/// Suggests `request` as `author`, announcing it when it is new.
///
/// # Errors
/// `404` for an unknown page; `409 page_is_a_learning` and `409 page_deleted`; `400` for edits that
/// do not apply, change nothing, or leave the page too long, and for arguments out of bounds; any
/// database error.
pub async fn suggest(
    database: &Pool,
    events: &EventBus,
    author: Author<'_>,
    request: SuggestPageRequest,
) -> Result<SuggestedPage, ApiError> {
    let new = NewSuggestion::new(
        request.id,
        Some(request.base_version),
        request.edits,
        &request.reason,
        request.quote.as_deref(),
        request.source.as_deref(),
    )?;
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let suggested = page_suggestion::suggest(&mut connection, new, author).await?;
    let created = suggested.created;
    let current = page::find(&mut connection, request.id).await?.body;
    let suggestion = SuggestionResponse::new(suggested.suggestion.clone(), &current);
    announce_suggested(events, &mut connection, suggested).await;
    Ok(SuggestedPage {
        suggestion,
        created,
        message: "Nothing on the page changes until a person accepts this suggestion on the \
            page's Suggestions tab.",
    })
}
