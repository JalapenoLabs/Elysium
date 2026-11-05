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
//! as one made in the browser, naming the agent acting for its person. They take the database and
//! the event bus rather than the app's state, since a turn's tools have those alone.

mod add_label;
mod create_page;
mod delete_page;
mod get_page;
mod get_passage;
mod get_version;
mod list_history;
mod list_pages;
mod list_versions;
mod move_page;
mod publish_page;
mod remove_label;
mod restore_page;
mod restore_version;
mod save_page;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;
use validator::{Validate, ValidationError, ValidationErrors};

pub use self::add_label::{PageLabelArguments, add as add_label};
pub use self::create_page::{CreatePageRequest, create};
pub use self::get_page::{PageArguments, read};
pub use self::list_pages::{ListPagesArguments, list_branch};
pub use self::move_page::{MovePageArguments, move_to};
pub use self::publish_page::{PublishPageRequest, publish};
pub use self::remove_label::remove as remove_label;
pub use self::save_page::{PAGE_IS_A_LEARNING, SavePageRequest, save};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::company_doc::Section;
use crate::models::page::{
    self, HistoryEntry, LearningFields, Page, PageEdit, PageSummary, PageVersion, Recorded,
    VersionSummary,
};
use crate::models::page_label;
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::action_items::validate_not_blank;
use crate::routes::v1::learnings::AgentResponse;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_pages::handle).post(create_page::handle))
        .route("/{id}", get(get_page::handle).delete(delete_page::handle))
        .route("/{id}/publish", post(publish_page::handle))
        .route("/{id}/move", post(move_page::handle))
        .route("/{id}/restore", post(restore_page::handle))
        .route("/{id}/labels", post(add_label::handle))
        .route("/{id}/labels/{label}", delete(remove_label::handle))
        .route("/{id}/passages/{position}", get(get_passage::handle))
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
    /// The key of the category a learning is filed under; `null` on a page that is no learning.
    category: Option<String>,
    /// What shows a learning is true; `null` on a page that is no learning.
    evidence: Option<String>,
    /// The newest version, which a publish names as `baseVersion`.
    current_version: i32,
    /// Agents may only propose changes to it.
    proposal_only: bool,
    /// One of the pages Elysium relies on: nobody moves, deletes, or renames it.
    fixed: bool,
    /// The labels it carries, in order. No part of a version.
    labels: Vec<String>,
    /// Who published the newest version, and when.
    published_by: Uuid,
    published_at: DateTime<Utc>,
    /// The agent that acted for them; `null` when they published it themselves.
    agent: Option<AgentResponse>,
    /// Who created it.
    created_by: Uuid,
    created_at: DateTime<Utc>,
    /// When what it holds last changed: published, deleted, or restored. A move or a label leaves
    /// it alone.
    updated_at: DateTime<Utc>,
    /// Set while it is deleted; it can be restored.
    deleted_at: Option<DateTime<Utc>>,
}

impl PageResponse {
    fn new(page: Page, published: VersionSummary, labels: Vec<String>) -> Self {
        Self {
            id: page.id,
            parent_id: page.parent_id,
            position: page.position,
            fixed: page::is_fixed(page.id),
            title: page.title,
            body: page.body,
            category: page.category,
            evidence: page.evidence,
            current_version: page.current_version,
            proposal_only: page.proposal_only,
            labels,
            published_by: published.created_by,
            published_at: published.created_at,
            agent: AgentResponse::new(published.agent, published.agent_name),
            created_by: page.created_by,
            created_at: page.created_at,
            updated_at: page.updated_at,
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
    /// One of the pages Elysium relies on: nobody moves, deletes, or renames it.
    fixed: bool,
    /// The labels it carries, in order: all a client needs to filter the tree, list a label's
    /// pages, and suggest the labels in use.
    labels: Vec<String>,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl PageSummaryResponse {
    fn new(page: PageSummary, labels: Vec<String>) -> Self {
        Self {
            fixed: page::is_fixed(page.id),
            id: page.id,
            parent_id: page.parent_id,
            position: page.position,
            title: page.title,
            current_version: page.current_version,
            labels,
            updated_at: page.updated_at,
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
    /// The category it was filed under, as it was then; `null` on a page that is no learning.
    category: Option<String>,
    /// What showed it was true; `null` on a page that is no learning.
    evidence: Option<String>,
}

impl From<PageVersion> for VersionResponse {
    fn from(version: PageVersion) -> Self {
        Self {
            summary: VersionSummaryResponse::from(version.summary),
            body: version.body,
            category: version.category,
            evidence: version.evidence,
        }
    }
}

/// A history entry as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryResponse {
    id: Uuid,
    page_id: Uuid,
    /// `created`, `published`, `moved`, `deleted`, `restored`, `labeled`, or `unlabeled`.
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
    /// A new page, as its version 1. Only an agent's `page_save` answers it: the browser's route
    /// for creating a page answers the page.
    Created,
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

/// A learning's category and evidence as a page is published with them, and what they may be.
#[derive(Debug, Validate)]
struct LearningText {
    #[validate(length(min = 1, max = 50))]
    category: String,
    #[validate(
        length(min = 1, max = 20000),
        custom(function = "validate_no_nul"),
        custom(function = "validate_not_blank")
    )]
    evidence: String,
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
        learning: None,
    })
}

/// What a learning's `category` and `evidence` publish as, trimmed as saving one trims them. Both or
/// neither: a learning holds both, and a publish that names neither leaves them as they are.
///
/// # Errors
/// What is wrong with either, keyed `category` and `evidence`, and either one given alone.
fn learning_fields(
    category: Option<&str>,
    evidence: Option<&str>,
) -> Result<Option<LearningFields>, ValidationErrors> {
    let (category, evidence) = match (category, evidence) {
        (None, None) => return Ok(None),
        (Some(category), Some(evidence)) => (category, evidence),
        (category, _) => {
            let missing = if category.is_none() {
                "category"
            } else {
                "evidence"
            };
            let mut errors = ValidationErrors::new();
            errors.add(
                missing,
                ValidationError::new("required").with_message(
                    "a learning's category and evidence are published together".into(),
                ),
            );
            return Err(errors);
        }
    };
    let text = LearningText {
        category: category.trim().to_owned(),
        evidence: evidence.trim().to_owned(),
    };
    text.validate()?;
    Ok(Some(LearningFields {
        category: text.category,
        evidence: text.evidence,
    }))
}

/// Postgres's text cannot hold a NUL, which JSON can carry, so it is refused here rather than
/// failing the write.
fn validate_no_nul(value: &str) -> Result<(), ValidationError> {
    if value.contains('\0') {
        return Err(ValidationError::new("nul").with_message("must not hold a NUL".into()));
    }
    Ok(())
}

/// Announces every page a write changed, as a page and as what else it stands for: clients showing
/// learnings and the company doc hear those events, not the tree's. A section is the doc changing.
pub fn announce(events: &EventBus, changed: &[Uuid], learnings: &[Uuid]) {
    for id in changed {
        events.publish(&ServerEvent::PageChanged { id: *id });
    }
    for id in learnings {
        events.publish(&ServerEvent::LearningChanged { id: *id });
    }
    if changed.iter().any(|id| Section::of_page(*id).is_some()) {
        events.publish(&ServerEvent::CompanyDocChanged);
    }
}

/// Announces every page a write changed, and answers the page it left.
async fn answer(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    recorded: Recorded,
) -> Result<PageResponse, ApiError> {
    announce(events, &recorded.changed, &recorded.learnings);
    response(connection, recorded.page).await
}

/// `page` as clients see it, with who published its newest version and the labels it carries.
async fn response(
    connection: &mut AsyncPgConnection,
    page: Page,
) -> Result<PageResponse, ApiError> {
    let published = page::version_summary(connection, page.id, page.current_version).await?;
    let labels = page_label::of_page(connection, page.id).await?;
    Ok(PageResponse::new(page, published, labels))
}

/// A write that publishes, answered with what it did.
async fn answer_published(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    recorded: Recorded,
) -> Result<PublishedPage, ApiError> {
    let outcome = if recorded.changed.is_empty() {
        PublishOutcome::Unchanged
    } else {
        PublishOutcome::Published
    };
    Ok(PublishedPage {
        page: answer(events, connection, recorded).await?,
        outcome,
    })
}
