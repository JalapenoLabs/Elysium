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
//!
//! A page's attachments are here too, and answer the same people. The rules for what a page takes
//! and how a file is served are `crate::page_attachments`.
//!
//! So are the edits agents suggest to a page, which a member reviews here, accepting them wholly or
//! in part or dismissing them with a reason (`crate::models::page_suggestion`). Agents suggest
//! through their tools ([`suggest`]); nothing creates a suggestion over HTTP, since a person edits a
//! page directly.

mod accept_suggestion;
mod add_label;
mod create_page;
mod delete_page;
mod dismiss_suggestion;
mod get_page;
mod get_passage;
mod get_version;
mod link_project;
mod list_history;
mod list_pages;
mod list_suggestions;
mod list_versions;
mod move_page;
#[cfg(test)]
mod project_link_tests;
mod publish_page;
mod read_attachment;
mod remove_attachment;
mod remove_label;
mod restore_attachment;
mod restore_page;
mod restore_version;
mod save_page;
mod suggest_page;
#[cfg(test)]
mod tests;
mod unlink_project;
mod upload_attachment;

use axum::Router;
use axum::extract::DefaultBodyLimit;
use axum::routing::{delete, get, post, put};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;
use validator::{Validate, ValidationError, ValidationErrors};

pub use self::add_label::{PageLabelArguments, add as add_label};
pub use self::create_page::{CreatePageRequest, create};
pub use self::get_page::{PageArguments, read_with_suggestions};
pub use self::link_project::{PageProjectArguments, link as link_project};
pub use self::list_pages::{ListPagesArguments, list_branch};
pub use self::move_page::{MovePageArguments, move_to};
pub use self::publish_page::{PublishPageRequest, publish};
pub use self::remove_label::remove as remove_label;
pub use self::save_page::{PAGE_IS_A_LEARNING, SavePageRequest, save};
pub use self::suggest_page::{SuggestPageRequest, suggest};
pub use self::unlink_project::unlink as unlink_project;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page::{
    self, COMPANY_PAGE_ID, HistoryEntry, LearningFields, Page, PageEdit, PageSummary, PageVersion,
    Recorded, VersionSummary,
};
use crate::models::page_attachment::{self, PageAttachment};
use crate::models::page_label;
use crate::models::page_project;
use crate::models::page_suggestion::{self, Decided, Suggested, Suggestion};
use crate::page_attachments::{self, MAX_ATTACHMENT_BYTES};
use crate::realtime::{EventBus, ServerEvent, Signal};
use crate::routes::v1::action_items::{publish_item_write, validate_not_blank};
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
        .route(
            "/{id}/attachments",
            // Files outgrow the API-wide body limit. The route refuses a larger one by its declared
            // length before a byte moves, and this holds a body that lied about it.
            post(upload_attachment::handle).layer(DefaultBodyLimit::max(
                usize::try_from(MAX_ATTACHMENT_BYTES).unwrap_or(usize::MAX),
            )),
        )
        .route(
            "/{id}/attachments/{attachment_id}",
            delete(remove_attachment::handle),
        )
        .route(
            "/{id}/attachments/{attachment_id}/restore",
            post(restore_attachment::handle),
        )
        .route(
            "/{id}/attachments/{attachment_id}/{name}",
            get(read_attachment::handle),
        )
        .route("/{id}/history", get(list_history::handle))
        .route("/{id}/suggestions", get(list_suggestions::handle))
        .route(
            "/{id}/suggestions/{suggestion_id}/accept",
            post(accept_suggestion::handle),
        )
        .route(
            "/{id}/suggestions/{suggestion_id}/dismiss",
            post(dismiss_suggestion::handle),
        )
        .route("/{id}/versions", get(list_versions::handle))
        .route("/{id}/versions/{version}", get(get_version::handle))
        .route(
            "/{id}/versions/{version}/restore",
            post(restore_version::handle),
        )
        .route(
            "/{id}/projects/{project_id}",
            put(link_project::handle).delete(unlink_project::handle),
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
    /// Agents may only suggest edits to it.
    proposal_only: bool,
    /// One of the pages Elysium relies on: nobody moves, deletes, or renames it.
    fixed: bool,
    /// The Company page or one of the company doc's sections, whose review a person can start.
    company_doc: bool,
    /// How many edits agents suggested to it wait for a person.
    pending_suggestions: i64,
    /// The project whose brief it is, which it stays linked to; `null` for every other page.
    brief_project_id: Option<Uuid>,
    /// The labels it carries, in order. No part of a version.
    labels: Vec<String>,
    /// The files it holds, the oldest first. No part of a version.
    attachments: Vec<AttachmentResponse>,
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
    /// The projects it is linked to, oldest link first.
    project_ids: Vec<Uuid>,
}

impl PageResponse {
    fn new(
        page: Page,
        published: VersionSummary,
        labels: Vec<String>,
        attachments: Vec<AttachmentResponse>,
        project_ids: Vec<Uuid>,
        pending_suggestions: i64,
    ) -> Self {
        Self {
            id: page.id,
            parent_id: page.parent_id,
            position: page.position,
            fixed: page.is_fixed(),
            company_doc: page.id == COMPANY_PAGE_ID || page.section().is_some(),
            pending_suggestions,
            brief_project_id: page.brief_project_id,
            title: page.title,
            body: page.body,
            category: page.category,
            evidence: page.evidence,
            current_version: page.current_version,
            proposal_only: page.proposal_only,
            labels,
            attachments,
            published_by: published.created_by,
            published_at: published.created_at,
            agent: AgentResponse::new(published.agent, published.agent_name),
            created_by: page.created_by,
            created_at: page.created_at,
            updated_at: page.updated_at,
            deleted_at: page.deleted_at,
            project_ids,
        }
    }
}

/// A file on a page as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentResponse {
    id: Uuid,
    page_id: Uuid,
    name: String,
    /// Where it is served, which a page's Markdown refers to it by.
    url: String,
    size_bytes: i64,
    /// Whether it displays in a page. The API decides, from a list of what is safe to: anything
    /// else is a link that downloads.
    is_image: bool,
    /// Who added it, and the agent that acted for them, if one did.
    created_by: Uuid,
    agent: Option<AgentResponse>,
    created_at: DateTime<Utc>,
}

impl From<PageAttachment> for AttachmentResponse {
    fn from(attachment: PageAttachment) -> Self {
        Self {
            url: page_attachments::address(
                attachment.page_id,
                attachment.id,
                &attachment.file_name,
            ),
            is_image: page_attachments::displays_inline(&attachment.content_type),
            id: attachment.id,
            page_id: attachment.page_id,
            name: attachment.file_name,
            size_bytes: attachment.size_bytes,
            created_by: attachment.created_by,
            agent: AgentResponse::new(attachment.agent, attachment.agent_name),
            created_at: attachment.created_at,
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
    /// The project whose brief it is, which it stays linked to; `null` for every other page.
    brief_project_id: Option<Uuid>,
    /// The labels it carries, in order: all a client needs to filter the tree, list a label's
    /// pages, and suggest the labels in use.
    labels: Vec<String>,
    /// How many edits agents suggested to it wait for a person, which the tree shows.
    pending_suggestions: i64,
    updated_at: DateTime<Utc>,
    deleted_at: Option<DateTime<Utc>>,
}

impl PageSummaryResponse {
    pub fn new(page: PageSummary, labels: Vec<String>, pending_suggestions: i64) -> Self {
        Self {
            pending_suggestions,
            fixed: page.is_fixed(),
            brief_project_id: page.brief_project_id,
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
    /// `created`, `published`, `moved`, `deleted`, `restored`, `labeled`, `unlabeled`, `attached`,
    /// `detached`, `project_linked`, or `project_unlinked`.
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
/// learnings hear those events, not the tree's.
pub fn announce(events: &EventBus, changed: &[Uuid], learnings: &[Uuid]) {
    for id in changed {
        events.publish(&ServerEvent::PageChanged { id: *id });
    }
    for id in learnings {
        events.publish(&ServerEvent::LearningChanged { id: *id });
    }
}

/// Announces a suggestion once it commits: its page, whose count of what waits moved, and the
/// page's inbox item it raised or retitled. A suggestion already pending changed nothing, so it
/// announces nothing. Whatever is suggesting, the suggestion stands, so an item that cannot be
/// announced is logged rather than failing it; a client that missed it reads it on its next load.
pub async fn announce_suggested(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    suggested: Suggested,
) {
    if !suggested.created {
        return;
    }
    let page_id = suggested.suggestion.page_id;
    events.publish(&ServerEvent::PageChanged { id: page_id });
    let at = suggested.suggestion.created_at;
    announce_review_item(events, connection, page_id, suggested.item, at).await;
}

/// Announces a decision on a suggestion once it commits, and answers the suggestion and its page as
/// they now read. The page's count of what waits moved whether or not a version was published, so
/// the page is announced either way, with whatever else the version reached and the Project
/// Manager a brief's version woke.
async fn answer_decided(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    decided: Decided,
) -> Result<(SuggestionResponse, PageResponse), ApiError> {
    let page_id = decided.suggestion.page_id;
    let at = decided.suggestion.decided_at.unwrap_or_else(Utc::now);
    let page = if let Some(recorded) = decided.published {
        answer(events, connection, recorded).await?
    } else {
        announce(events, &[page_id], &[]);
        let page = page::find(connection, page_id).await?;
        response(connection, page).await?
    };
    announce_review_item(events, connection, page_id, decided.item, at).await;
    let suggestion = SuggestionResponse::new(decided.suggestion, &page.body);
    Ok((suggestion, page))
}

/// Announces the inbox item a suggestion's write kept in step at `at`, logging one that cannot be.
async fn announce_review_item(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    item: Option<
        crate::models::action_item_event::Recorded<crate::models::action_item::ActionItem>,
    >,
    at: DateTime<Utc>,
) {
    let Some(item) = item else {
        return;
    };
    let item_id = item.record.id;
    if let Err(error) = publish_item_write(events, connection, item, &[], at).await {
        tracing::event!(
            name: "pages.suggestions.item_unpublished",
            tracing::Level::WARN,
            page.id = %page_id,
            action_item.id = %item_id,
            error.message = %error,
            "a page's review item was written but could not be announced",
        );
    }
}

/// Announces every page a write changed, and the Project Manager it woke, and answers the page it
/// left.
async fn answer(
    events: &EventBus,
    connection: &mut AsyncPgConnection,
    recorded: Recorded,
) -> Result<PageResponse, ApiError> {
    announce(events, &recorded.changed, &recorded.learnings);
    if let Some(project_id) = recorded.woke {
        events.publish(&ServerEvent::ProjectManagerChanged { project_id });
        events.signal(&Signal::ProjectManagers);
    }
    response(connection, recorded.page).await
}

/// `page` as clients see it, with who published its newest version, the labels and files it
/// carries, and the projects it is linked to.
async fn response(
    connection: &mut AsyncPgConnection,
    page: Page,
) -> Result<PageResponse, ApiError> {
    let published = page::version_summary(connection, page.id, page.current_version).await?;
    let labels = page_label::of_page(connection, page.id).await?;
    let attachments = page_attachment::of_page(connection, page.id)
        .await?
        .into_iter()
        .map(AttachmentResponse::from)
        .collect();
    let project_ids = page_project::projects_of(connection, page.id).await?;
    let pending_suggestions = page_suggestion::pending_counts(connection, Some(&[page.id]))
        .await?
        .remove(&page.id)
        .unwrap_or_default();
    Ok(PageResponse::new(
        page,
        published,
        labels,
        attachments,
        project_ids,
        pending_suggestions,
    ))
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

/// A suggestion as clients and agents see it: its changes rather than its text, so a long page's
/// suggestion reads as what it changes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuggestionResponse {
    id: Uuid,
    page_id: Uuid,
    /// The version it was written from.
    base_version: i32,
    /// What it changes, in order, each numbered from 0 as accepting one in part names it.
    changes: Vec<ChangeResponse>,
    reason: String,
    quote: Option<String>,
    source: Option<String>,
    /// The timeline record the company doc's review read it in, while that record exists.
    timeline_record_id: Option<Uuid>,
    /// The company doc review's run that suggested it, while that run exists.
    workflow_run_id: Option<Uuid>,
    /// `pending`, `accepted`, or `dismissed`.
    state: String,
    /// The changes an acceptance took; `null` for all of them, or while it is not accepted.
    accepted_changes: Option<Vec<i32>>,
    /// Why it was dismissed.
    dismissal_reason: Option<String>,
    decided_by: Option<Uuid>,
    decided_at: Option<DateTime<Utc>>,
    /// Who suggested it: a person, or the machine user of Elysia or a Project Manager.
    created_by: Uuid,
    /// The agent acting for them, if one was named.
    agent: Option<AgentResponse>,
    created_at: DateTime<Utc>,
}

/// One change a suggestion makes, with whether it would still merge with the page as it reads now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangeResponse {
    /// Its place among the suggestion's changes, from 0.
    index: usize,
    /// Its first line in the version the suggestion was written from, from 1.
    line: usize,
    /// The lines it changes, with a few unchanged ones on each side, before and after it.
    before: String,
    after: String,
    /// Whether accepting it alone would merge with the page now; `false` when the page changed
    /// since, in the same lines.
    applies: bool,
}

impl SuggestionResponse {
    /// `suggestion` against a page that now holds `current`.
    pub fn new(suggestion: Suggestion, current: &str) -> Self {
        let applies = page_suggestion::applies(&suggestion, current);
        let changes = page_suggestion::changes(&suggestion.base_body, &suggestion.body)
            .into_iter()
            .zip(applies)
            .enumerate()
            .map(|(index, (change, applies))| ChangeResponse {
                index,
                line: change.line,
                before: change.before,
                after: change.after,
                applies,
            })
            .collect();
        Self {
            id: suggestion.id,
            page_id: suggestion.page_id,
            base_version: suggestion.base_version,
            changes,
            reason: suggestion.reason,
            quote: suggestion.quote,
            source: suggestion.source,
            timeline_record_id: suggestion.timeline_record_id,
            workflow_run_id: suggestion.workflow_run_id,
            state: suggestion.state,
            accepted_changes: suggestion
                .accepted_changes
                .map(|chosen| chosen.into_iter().flatten().collect()),
            dismissal_reason: suggestion.dismissal_reason,
            decided_by: suggestion.decided_by,
            decided_at: suggestion.decided_at,
            created_by: suggestion.created_by,
            agent: AgentResponse::new(suggestion.agent, suggestion.agent_name),
            created_at: suggestion.created_at,
        }
    }
}

/// The suggestions waiting on page `id`, oldest first, each read against what the page holds now:
/// what the route answers and `page_read` adds to a page.
///
/// # Errors
/// `404` for an unknown page, and any database error.
pub async fn pending_suggestions(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> Result<Vec<SuggestionResponse>, ApiError> {
    let page = page::find(connection, id).await?;
    let pending = page_suggestion::pending(connection, id).await?;
    Ok(pending
        .into_iter()
        .map(|suggestion| SuggestionResponse::new(suggestion, &page.body))
        .collect())
}
