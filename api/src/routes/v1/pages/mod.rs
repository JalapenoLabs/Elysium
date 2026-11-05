// Copyright © 2026 Jalapeno Labs

//! `/api/v1/pages`: the knowledge base's pages, one tree for the whole workspace, every version
//! kept. See `docs/knowledge.md`, Pages.
//!
//! Every route answers the company's members alone ([`MemberUser`]): pages are part of the
//! knowledge base, which a guest never reaches (`docs/knowledge.md`, Who sees what). Anyone else
//! reads and edits any page.
//!
//! Each route's work is a public function taking the [`Author`] of the change, so an agent's tool
//! calls what the route calls and its change is validated, recorded, indexed, and announced exactly
//! as one made in the browser, naming the agent acting for its person.

mod create_page;
mod delete_page;
mod get_page;
mod get_version;
mod list_history;
mod list_pages;
mod list_versions;
mod move_page;
mod publish_page;
mod restore_page;
mod restore_version;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;
use validator::{Validate, ValidationError, ValidationErrors};

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page::{
    self, HistoryEntry, Page, PageEdit, PageSummary, PageVersion, Recorded, VersionSummary,
};
use crate::realtime::ServerEvent;
use crate::routes::v1::learnings::AgentResponse;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_pages::handle).post(create_page::handle))
        .route("/{id}", get(get_page::handle).delete(delete_page::handle))
        .route("/{id}/publish", post(publish_page::handle))
        .route("/{id}/move", post(move_page::handle))
        .route("/{id}/restore", post(restore_page::handle))
        .route("/{id}/history", get(list_history::handle))
        .route("/{id}/versions", get(list_versions::handle))
        .route("/{id}/versions/{version}", get(get_version::handle))
        .route(
            "/{id}/versions/{version}/restore",
            post(restore_version::handle),
        )
}

/// A page as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageResponse {
    id: Uuid,
    /// The page it sits under; `null` at the top of the tree.
    parent_id: Option<Uuid>,
    /// Its place among its siblings, lowest first. Only its order means anything.
    position: i64,
    title: String,
    /// Markdown.
    body: String,
    /// The newest version, which a publish names as `baseVersion`.
    current_version: i32,
    /// Agents may only propose changes to it.
    proposal_only: bool,
    /// Who published the newest version, and when.
    published_by: Uuid,
    published_at: DateTime<Utc>,
    /// The agent that acted for them; `null` when they published it themselves.
    agent: Option<AgentResponse>,
    /// Who created it.
    created_by: Uuid,
    created_at: DateTime<Utc>,
    /// Set while it is deleted; it can be restored.
    deleted_at: Option<DateTime<Utc>>,
}

impl PageResponse {
    fn new(page: Page, published: VersionSummary) -> Self {
        Self {
            id: page.id,
            parent_id: page.parent_id,
            position: page.position,
            title: page.title,
            body: page.body,
            current_version: page.current_version,
            proposal_only: page.proposal_only,
            published_by: published.created_by,
            published_at: published.created_at,
            agent: AgentResponse::new(published.agent, published.agent_name),
            created_by: page.created_by,
            created_at: page.created_at,
            deleted_at: page.deleted_at,
        }
    }
}

/// A page as the tree lists it, without what it says.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSummaryResponse {
    id: Uuid,
    parent_id: Option<Uuid>,
    position: i64,
    title: String,
    current_version: i32,
    deleted_at: Option<DateTime<Utc>>,
}

impl From<PageSummary> for PageSummaryResponse {
    fn from(page: PageSummary) -> Self {
        Self {
            id: page.id,
            parent_id: page.parent_id,
            position: page.position,
            title: page.title,
            current_version: page.current_version,
            deleted_at: page.deleted_at,
        }
    }
}

/// A version as clients list it, without what it says.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionSummaryResponse {
    page_id: Uuid,
    version: i32,
    title: String,
    /// The version a restore copied; `null` for one published as written.
    restored_from: Option<i32>,
    /// Who published it.
    created_by: Uuid,
    /// The agent that acted for them, if one did.
    agent: Option<AgentResponse>,
    created_at: DateTime<Utc>,
}

impl From<VersionSummary> for VersionSummaryResponse {
    fn from(version: VersionSummary) -> Self {
        Self {
            page_id: version.page_id,
            version: version.version,
            title: version.title,
            restored_from: version.restored_from,
            created_by: version.created_by,
            agent: AgentResponse::new(version.agent, version.agent_name),
            created_at: version.created_at,
        }
    }
}

/// A version whole.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionResponse {
    #[serde(flatten)]
    summary: VersionSummaryResponse,
    /// Markdown.
    body: String,
}

impl From<PageVersion> for VersionResponse {
    fn from(version: PageVersion) -> Self {
        Self {
            summary: VersionSummaryResponse::from(version.summary),
            body: version.body,
        }
    }
}

/// A history entry as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryResponse {
    id: Uuid,
    page_id: Uuid,
    /// `created`, `published`, `moved`, `deleted`, or `restored`.
    kind: String,
    /// Who made the change.
    created_by: Uuid,
    /// The agent that acted for them, if one did.
    agent: Option<AgentResponse>,
    /// Shaped by `kind`; see `docs/knowledge.md`, Pages.
    data: Value,
    created_at: DateTime<Utc>,
}

impl From<HistoryEntry> for HistoryEntryResponse {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id,
            page_id: entry.page_id,
            kind: entry.kind,
            created_by: entry.created_by,
            agent: AgentResponse::new(entry.agent, entry.agent_name),
            data: entry.data,
            created_at: entry.created_at,
        }
    }
}

/// What a publish did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PublishOutcome {
    Published,
    /// The page already said exactly this; no version was made.
    Unchanged,
}

/// A page and what publishing it did.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishedPage {
    pub page: PageResponse,
    pub outcome: PublishOutcome,
}

/// Who a write through these routes is by: the member, on their own, now.
fn author(member: &MemberUser) -> Author<'static> {
    Author {
        user_id: member.id(),
        agent: None,
        at: Utc::now(),
    }
}

/// A title and body as they are stored, and what they may be.
#[derive(Debug, Validate)]
struct PageText {
    #[validate(length(min = 1, max = 300), custom(function = "validate_no_nul"))]
    title: String,
    #[validate(length(max = 100_000), custom(function = "validate_no_nul"))]
    body: String,
}

/// What `title` and `body` publish as, once normalized and checked.
///
/// The title is trimmed and its whitespace collapsed. The body's line endings are normalized and
/// its trailing whitespace dropped, so an editor that writes a page back with a trailing newline or
/// Windows line endings has changed nothing; nothing else about its Markdown is touched.
///
/// # Errors
/// What is wrong with either, keyed `title` and `body`.
fn page_edit(title: &str, body: &str) -> Result<PageEdit, ValidationErrors> {
    let text = PageText {
        title: title.split_whitespace().collect::<Vec<_>>().join(" "),
        body: body.replace("\r\n", "\n").trim_end().to_owned(),
    };
    text.validate()?;
    Ok(PageEdit {
        title: text.title,
        body: text.body,
    })
}

/// Postgres's text cannot hold a NUL, which JSON can carry, so it is refused here rather than
/// failing the write.
fn validate_no_nul(value: &str) -> Result<(), ValidationError> {
    if value.contains('\0') {
        return Err(ValidationError::new("nul").with_message("must not hold a NUL".into()));
    }
    Ok(())
}

/// Announces every page a write changed, and answers the page it left.
async fn answer(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    recorded: Recorded,
) -> Result<PageResponse, ApiError> {
    for id in recorded.changed {
        state.events.publish(&ServerEvent::PageChanged { id });
    }
    response(connection, recorded.page).await
}

/// `page` as clients see it, with who published its newest version.
async fn response(
    connection: &mut AsyncPgConnection,
    page: Page,
) -> Result<PageResponse, ApiError> {
    let published = page::version_summary(connection, page.id, page.current_version).await?;
    Ok(PageResponse::new(page, published))
}

/// A write that publishes, answered with what it did.
async fn answer_published(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    recorded: Recorded,
) -> Result<PublishedPage, ApiError> {
    let outcome = if recorded.changed.is_empty() {
        PublishOutcome::Unchanged
    } else {
        PublishOutcome::Published
    };
    Ok(PublishedPage {
        page: answer(state, connection, recorded).await?,
        outcome,
    })
}
