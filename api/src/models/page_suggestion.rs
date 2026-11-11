// Copyright © 2026 Jalapeno Labs

//! Edits suggested to a knowledge base page, waiting for a person to accept or dismiss them. See
//! `docs/knowledge.md`, Pages, Suggestions.
//!
//! Any agent may suggest an edit to any page but a learning ([`suggest`]): Elysia, a Project
//! Manager, a coding or Studio session, or a person's MCP client. A suggestion is the text the page
//! would hold, made from the version the agent read, and nothing changes until a person accepts it.
//! The company doc's sections are pages agents only suggest to, and `crate::models::company_doc`
//! adds nothing to how a suggestion to one is made, reviewed, merged, or recorded.
//!
//! A suggestion is read as its [`changes`]: each run of lines it adds, removes, or replaces against
//! the version it was written from. A person accepts every change or picks some ([`accept`]): the
//! chosen ones are applied to that version, merged three ways with what the page holds now, as git
//! merges a branch, and published as the page's next version, naming the suggestion. A change whose
//! lines were changed since is refused rather than undoing someone's edit. Or the person dismisses
//! it, saying why ([`dismiss`]), which leaves the page alone.
//!
//! Every write that adds or settles a pending suggestion keeps its page's one inbox item in step in
//! the same transaction ([`review_item`]), so people are told once per page, not once per
//! suggestion.

pub mod review_item;

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::dsl::count_star;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use similar::{Algorithm, DiffOp};
use uuid::Uuid;

use crate::database::schema::page_suggestions;
use crate::models::action_item::ActionItem;
use crate::models::action_item_event::Recorded as RecordedItem;
use crate::models::agent::{Agent, Author};
use crate::models::company_doc;
use crate::models::page::{self, Page, PageContent, PageError, Provenance, Recorded};

/// The most edits one suggestion holds: a page's worth of corrections, each small enough to
/// review.
pub const EDITS_MAX: usize = 20;

/// The longest reason, quote, source, and dismissal reason, as `page_suggestions` allows.
pub const REASON_MAX_CHARACTERS: usize = 2_000;
pub const QUOTE_MAX_CHARACTERS: usize = 2_000;
pub const SOURCE_MAX_CHARACTERS: usize = 500;

/// The longest a page's body may be, in characters, as the page routes allow, and so the longest
/// text a suggestion may leave it with. A company doc section's own bound is
/// [`company_doc::CONTENT_MAX_CHARACTERS`].
pub const BODY_MAX_CHARACTERS: usize = 100_000;

/// How many unchanged lines a change shows on each side, so a person reads it where it sits.
const CONTEXT_LINES: usize = 2;

/// Where a suggestion stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SuggestionState {
    Pending,
    Accepted,
    Dismissed,
}

impl SuggestionState {
    /// The name stored in `state` and sent to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Accepted => "accepted",
            Self::Dismissed => "dismissed",
        }
    }
}

/// A stored suggestion.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = page_suggestions, check_for_backend(diesel::pg::Pg))]
pub struct Suggestion {
    pub id: Uuid,
    pub page_id: Uuid,
    /// The version it was written from, and that version's text, which its changes are read
    /// against and which accepting merges from.
    pub base_version: i32,
    pub base_body: String,
    /// The page's text as the suggestion would leave it.
    pub body: String,
    pub reason: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    pub timeline_record_id: Option<Uuid>,
    pub workflow_run_id: Option<Uuid>,
    /// A [`SuggestionState`] name.
    pub state: String,
    /// The changes an acceptance took, by their place among [`changes`]; `None` for all of them.
    /// Postgres arrays hold nullable elements, which `page_suggestions` refuses.
    pub accepted_changes: Option<Vec<Option<i32>>>,
    pub dismissal_reason: Option<String>,
    pub decided_by: Option<Uuid>,
    pub decided_at: Option<DateTime<Utc>>,
    /// Who suggested it: the person, or the machine user of Elysia or a Project Manager.
    pub created_by: Uuid,
    /// The agent acting for them, in its recorded form.
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Suggestion {
    /// Whether it still waits for a person.
    pub fn is_pending(&self) -> bool {
        self.state == SuggestionState::Pending.as_str()
    }
}

#[derive(Insertable)]
#[diesel(table_name = page_suggestions)]
struct SuggestionRow<'a> {
    id: Uuid,
    page_id: Uuid,
    base_version: i32,
    base_body: &'a str,
    body: &'a str,
    reason: &'a str,
    quote: Option<&'a str>,
    source: Option<&'a str>,
    timeline_record_id: Option<Uuid>,
    workflow_run_id: Option<Uuid>,
    created_by: Uuid,
    agent: Option<&'a str>,
    agent_name: Option<&'a str>,
    created_at: DateTime<Utc>,
}

/// One change to a page's text, in the form Claude Code's own Edit tool takes: `find` is text the
/// version holds exactly once, and `replace` takes its place. An empty `find` adds `replace` at the
/// end, on a line of its own.
///
/// Agents suggest edits rather than a page's whole text, so an agent that misreads a long page
/// cannot drop what it did not mean to change.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Edit {
    /// Text the page holds exactly once, to be replaced; empty to add to the end.
    pub find: String,
    /// What takes its place; empty to remove it.
    pub replace: String,
}

/// A suggestion to make, checked and trimmed ([`NewSuggestion::new`]).
#[derive(Debug, Clone)]
pub struct NewSuggestion {
    pub page_id: Uuid,
    /// The version the edits were written against; `None` for the page as it reads now.
    pub base_version: Option<i32>,
    /// Applied in order to that version's text.
    pub edits: Vec<Edit>,
    pub reason: String,
    pub quote: Option<String>,
    pub source: Option<String>,
    /// The timeline record it was read in, for the company doc review's suggestions.
    pub timeline_record_id: Option<Uuid>,
    /// The company doc review's run that made it.
    pub workflow_run_id: Option<Uuid>,
}

impl NewSuggestion {
    /// A suggestion of `edits` to page `page_id`, made against `base_version`, with its reason,
    /// quote, and source trimmed, a blank quote or source dropped.
    ///
    /// # Errors
    /// [`SuggestionError::Invalid`] for no edits, too many, a blank reason, or a field past its
    /// bound, saying which.
    pub fn new(
        page_id: Uuid,
        base_version: Option<i32>,
        edits: Vec<Edit>,
        reason: &str,
        quote: Option<&str>,
        source: Option<&str>,
    ) -> Result<Self, SuggestionError> {
        if edits.is_empty() {
            return Err(SuggestionError::Invalid(
                "a suggestion holds at least one edit".to_owned(),
            ));
        }
        if edits.len() > EDITS_MAX {
            return Err(SuggestionError::Invalid(format!(
                "a suggestion holds at most {EDITS_MAX} edits; suggest the rest separately"
            )));
        }
        Ok(Self {
            page_id,
            base_version,
            edits,
            reason: required_text(reason, REASON_MAX_CHARACTERS, "reason")?,
            quote: optional_text(quote, QUOTE_MAX_CHARACTERS, "quote")?,
            source: optional_text(source, SOURCE_MAX_CHARACTERS, "source")?,
            timeline_record_id: None,
            workflow_run_id: None,
        })
    }
}

/// `text` trimmed, refused when blank or past `max` characters.
fn required_text(text: &str, max: usize, field: &str) -> Result<String, SuggestionError> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max {
        return Err(SuggestionError::Invalid(format!(
            "the {field} is 1 to {max} characters"
        )));
    }
    Ok(text.to_owned())
}

/// `text` trimmed, `None` when blank, refused when past `max` characters.
fn optional_text(
    text: Option<&str>,
    max: usize,
    field: &str,
) -> Result<Option<String>, SuggestionError> {
    let Some(text) = text.map(str::trim).filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    if text.chars().count() > max {
        return Err(SuggestionError::Invalid(format!(
            "the {field} is at most {max} characters"
        )));
    }
    Ok(Some(text.to_owned()))
}

/// A suggestion, whether suggesting stored it or found the same change already pending, and the
/// page's inbox item it raised or retitled, for the caller to announce once the write commits.
#[derive(Debug)]
pub struct Suggested {
    pub suggestion: Suggestion,
    pub created: bool,
    pub item: Option<RecordedItem<ActionItem>>,
}

/// What a decision on a suggestion did: the suggestion as decided, the version an acceptance
/// published (`None` when the page already held it, or for a dismissal), and the page's inbox item
/// it updated or resolved.
#[derive(Debug)]
pub struct Decided {
    pub suggestion: Suggestion,
    pub published: Option<Recorded>,
    pub item: Option<RecordedItem<ActionItem>>,
}

/// Why a suggestion, or a decision on one, was refused.
#[derive(Debug, thiserror::Error)]
pub enum SuggestionError {
    /// Includes [`DieselError::NotFound`] for an unknown page, or a suggestion not on it.
    #[error(transparent)]
    Database(#[from] DieselError),
    /// The page refused the write: deleted, or its body out of bounds for what it is.
    #[error(transparent)]
    Page(PageError),
    /// Out of bounds, such as a suggestion with no edits; the message says which bound.
    #[error("{0}")]
    Invalid(String),
    /// A learning is changed through `learning_save`, with its evidence.
    #[error("a learning is changed with learning_save, not suggested to")]
    IsALearning,
    /// The version a suggestion names is not one the page has.
    #[error("the page has no version {0}; read it again and suggest against its currentVersion")]
    UnknownVersion(i32),
    /// An edit's `find` is not in the version, or is there more than once; the message says
    /// which, for the agent to read the page again.
    #[error("{0}")]
    EditDoesNotApply(String),
    #[error("this suggestion leaves the page as it already reads")]
    ChangesNothing,
    #[error("the page would be longer than {max} characters")]
    TooLong { max: usize },
    #[error("this suggestion was already {0}")]
    AlreadyDecided(&'static str),
    /// A decision named a change the suggestion does not have, or none.
    #[error("{0}")]
    UnknownChange(String),
    /// The page changed since the suggestion was written, in the same lines a chosen change does.
    #[error(
        "the page changed since this was suggested, in the same lines; take the other changes, \
         dismiss it, or make the change by hand"
    )]
    Overlaps,
}

impl From<PageError> for SuggestionError {
    fn from(error: PageError) -> Self {
        match error {
            PageError::Database(error) => Self::Database(error),
            other => Self::Page(other),
        }
    }
}

/// Suggests `new` for a person to review, as `author`: its edits applied to the version it names,
/// or to the page as it reads now. The same change already pending on the page is answered rather
/// than stored again.
///
/// # Errors
/// `NotFound` for an unknown page; [`SuggestionError::IsALearning`]; [`PageError::Deleted`] for a
/// deleted page; [`SuggestionError::UnknownVersion`]; [`SuggestionError::EditDoesNotApply`] for an
/// edit whose text the version does not hold once; [`SuggestionError::ChangesNothing`] for edits
/// that leave the version, or the page as it reads now, as it is; [`SuggestionError::TooLong`];
/// any database error.
pub async fn suggest(
    connection: &mut AsyncPgConnection,
    new: NewSuggestion,
    author: Author<'_>,
) -> Result<Suggested, SuggestionError> {
    connection
        .transaction(async move |connection| {
            let page = page::find(connection, new.page_id).await?;
            if page.is_learning() {
                return Err(SuggestionError::IsALearning);
            }
            if page.deleted_at.is_some() {
                return Err(PageError::Deleted.into());
            }
            let base_version = new.base_version.unwrap_or(page.current_version);
            let base_body = base_text(connection, &page, base_version).await?;
            let body = apply_edits(&base_body, &new.edits)?;
            if body == base_body || body == page.body {
                return Err(SuggestionError::ChangesNothing);
            }
            let max = body_max(&page);
            if body.chars().count() > max {
                return Err(SuggestionError::TooLong { max });
            }

            let inserted = diesel::insert_into(page_suggestions::table)
                .values(SuggestionRow {
                    id: Uuid::now_v7(),
                    page_id: page.id,
                    base_version,
                    base_body: &base_body,
                    body: &body,
                    reason: &new.reason,
                    quote: new.quote.as_deref(),
                    source: new.source.as_deref(),
                    timeline_record_id: new.timeline_record_id,
                    workflow_run_id: new.workflow_run_id,
                    created_by: author.user_id,
                    agent: author.agent.map(Agent::id),
                    agent_name: author.agent.and_then(|agent| agent.name()),
                    created_at: author.at,
                })
                // `page_suggestions_pending_once` is partial, which no conflict target can name.
                .on_conflict_do_nothing()
                .returning(Suggestion::as_returning())
                .get_result(connection)
                .await
                .optional()?;
            let Some(suggestion) = inserted else {
                let pending = page_suggestions::table
                    .filter(page_suggestions::page_id.eq(page.id))
                    .filter(page_suggestions::state.eq(SuggestionState::Pending.as_str()))
                    .filter(page_suggestions::body.eq(&body))
                    .select(Suggestion::as_select())
                    .first(connection)
                    .await?;
                return Ok(Suggested {
                    suggestion: pending,
                    created: false,
                    item: None,
                });
            };
            let item =
                review_item::refresh(connection, &page, review_item::Cause::Suggested, author.at)
                    .await?;
            Ok(Suggested {
                suggestion,
                created: true,
                item,
            })
        })
        .await
}

/// The text of `page`'s version `version`: what it holds now for its current one.
async fn base_text(
    connection: &mut AsyncPgConnection,
    page: &Page,
    version: i32,
) -> Result<String, SuggestionError> {
    if version == page.current_version {
        return Ok(page.body.clone());
    }
    match page::find_version(connection, page.id, version).await {
        Ok(found) => Ok(found.body),
        Err(DieselError::NotFound) => Err(SuggestionError::UnknownVersion(version)),
        Err(error) => Err(error.into()),
    }
}

/// The longest text `page` may hold: a company doc section's bound, or any page's.
fn body_max(page: &Page) -> usize {
    if page.section().is_some() {
        return company_doc::CONTENT_MAX_CHARACTERS;
    }
    BODY_MAX_CHARACTERS
}

/// The suggestions waiting on page `page_id`, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn pending(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
) -> QueryResult<Vec<Suggestion>> {
    pending_on(connection, &[page_id]).await
}

/// The suggestions waiting on any of `page_ids`, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn pending_on(
    connection: &mut AsyncPgConnection,
    page_ids: &[Uuid],
) -> QueryResult<Vec<Suggestion>> {
    page_suggestions::table
        .filter(page_suggestions::page_id.eq_any(page_ids))
        .filter(page_suggestions::state.eq(SuggestionState::Pending.as_str()))
        .order(page_suggestions::id.asc())
        .select(Suggestion::as_select())
        .load(connection)
        .await
}

/// The newest `newest` suggestions in `state` on any of `page_ids`, oldest first, as the company
/// doc's review reads what waits and what people dismissed.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_on(
    connection: &mut AsyncPgConnection,
    page_ids: &[Uuid],
    state: SuggestionState,
    newest: i64,
) -> QueryResult<Vec<Suggestion>> {
    let mut latest: Vec<Suggestion> = page_suggestions::table
        .filter(page_suggestions::page_id.eq_any(page_ids))
        .filter(page_suggestions::state.eq(state.as_str()))
        .order(page_suggestions::id.desc())
        .limit(newest)
        .select(Suggestion::as_select())
        .load(connection)
        .await?;
    latest.reverse();
    Ok(latest)
}

/// How many suggestions wait on each page that has any: on every page, or only on `page_ids`. A
/// page with none is left out.
///
/// # Errors
/// Propagates any database error.
pub async fn pending_counts(
    connection: &mut AsyncPgConnection,
    page_ids: Option<&[Uuid]>,
) -> QueryResult<HashMap<Uuid, i64>> {
    let query = page_suggestions::table
        .filter(page_suggestions::state.eq(SuggestionState::Pending.as_str()))
        .group_by(page_suggestions::page_id)
        .select((page_suggestions::page_id, count_star()));
    let counts: Vec<(Uuid, i64)> = match page_ids {
        Some(page_ids) => {
            query
                .filter(page_suggestions::page_id.eq_any(page_ids))
                .load(connection)
                .await?
        }
        None => query.load(connection).await?,
    };
    Ok(counts.into_iter().collect())
}

/// Accepts suggestion `id` on page `page_id` as `author`: every change, or the ones `chosen` names
/// by their place among [`changes`], applied to the version it was written from, merged with what
/// the page holds now, and published as the page's next version naming the suggestion.
///
/// Changes the page already holds are accepted without a version.
///
/// # Errors
/// `NotFound` for a suggestion not on the page; [`SuggestionError::AlreadyDecided`];
/// [`PageError::Deleted`] for a deleted page; [`SuggestionError::UnknownChange`] for a choice of no
/// change or one the suggestion lacks; [`SuggestionError::Overlaps`] for a chosen change whose lines
/// the page changed since; [`SuggestionError::TooLong`] for a merge past the page's bound; the
/// refusals of [`page::publish_content`]; any database error.
pub async fn accept(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    id: Uuid,
    chosen: Option<Vec<usize>>,
    author: Author<'_>,
) -> Result<Decided, SuggestionError> {
    connection
        .transaction(async move |connection| {
            let suggestion = lock_pending(connection, page_id, id).await?;
            let page = page::lock_to_publish(connection, page_id).await?;
            if page.deleted_at.is_some() {
                return Err(PageError::Deleted.into());
            }
            let all = changes(&suggestion.base_body, &suggestion.body);
            let chosen = checked_choice(chosen, all.len())?;
            let taken = take(&suggestion.base_body, &suggestion.body, chosen.as_deref());
            let merged = merged(&suggestion.base_body, &page.body, &taken)
                .ok_or(SuggestionError::Overlaps)?;
            let max = body_max(&page);
            if merged.chars().count() > max {
                return Err(SuggestionError::TooLong { max });
            }

            let accepted_changes = chosen.map(|chosen| {
                chosen
                    .into_iter()
                    .map(|index| i32::try_from(index).ok())
                    .collect()
            });
            let suggestion = decide(
                connection,
                id,
                Decision::Accepted { accepted_changes },
                author,
            )
            .await?;
            let published = if merged == page.body {
                None
            } else {
                let content = PageContent {
                    body: merged,
                    ..page.content()
                };
                let provenance = Provenance {
                    suggestion_id: Some(id),
                    ..Provenance::default()
                };
                Some(page::publish_content(connection, &page, content, provenance, author).await?)
            };
            let item =
                review_item::refresh(connection, &page, review_item::Cause::Settled, author.at)
                    .await?;
            Ok(Decided {
                suggestion,
                published,
                item,
            })
        })
        .await
}

/// Dismisses suggestion `id` on page `page_id` as `author`, saying why. The page is left as it is,
/// so nothing is recorded in its history; the suggestion keeps who dismissed it, when, and why.
///
/// # Errors
/// `NotFound` for a suggestion not on the page; [`SuggestionError::AlreadyDecided`];
/// [`SuggestionError::Invalid`] for a blank reason or one past its bound; any database error.
pub async fn dismiss(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    id: Uuid,
    reason: &str,
    author: Author<'_>,
) -> Result<Decided, SuggestionError> {
    let reason = required_text(reason, REASON_MAX_CHARACTERS, "reason")?;
    connection
        .transaction(async move |connection| {
            lock_pending(connection, page_id, id).await?;
            let suggestion = decide(connection, id, Decision::Dismissed { reason }, author).await?;
            let page = page::find(connection, page_id).await?;
            let item =
                review_item::refresh(connection, &page, review_item::Cause::Settled, author.at)
                    .await?;
            Ok(Decided {
                suggestion,
                published: None,
                item,
            })
        })
        .await
}

/// Locks suggestion `id` on page `page_id`, refusing one already decided.
async fn lock_pending(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    id: Uuid,
) -> Result<Suggestion, SuggestionError> {
    let suggestion: Suggestion = page_suggestions::table
        .find(id)
        .filter(page_suggestions::page_id.eq(page_id))
        .for_update()
        .select(Suggestion::as_select())
        .first(connection)
        .await?;
    match suggestion.state.as_str() {
        "pending" => Ok(suggestion),
        "accepted" => Err(SuggestionError::AlreadyDecided("accepted")),
        _ => Err(SuggestionError::AlreadyDecided("dismissed")),
    }
}

/// How a person decided.
#[derive(Debug)]
enum Decision {
    Accepted {
        accepted_changes: Option<Vec<Option<i32>>>,
    },
    Dismissed {
        reason: String,
    },
}

async fn decide(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    decision: Decision,
    author: Author<'_>,
) -> QueryResult<Suggestion> {
    let (state, accepted_changes, dismissal_reason) = match decision {
        Decision::Accepted { accepted_changes } => {
            (SuggestionState::Accepted, accepted_changes, None)
        }
        Decision::Dismissed { reason } => (SuggestionState::Dismissed, None, Some(reason)),
    };
    diesel::update(page_suggestions::table.find(id))
        .set((
            page_suggestions::state.eq(state.as_str()),
            page_suggestions::accepted_changes.eq(accepted_changes),
            page_suggestions::dismissal_reason.eq(dismissal_reason),
            page_suggestions::decided_by.eq(author.user_id),
            page_suggestions::decided_at.eq(author.at),
        ))
        .returning(Suggestion::as_returning())
        .get_result(connection)
        .await
}

/// `chosen` as a sorted set of the places of changes among `count`, or `None` when it names every
/// one, so an acceptance of all of them records it as such however it was asked for.
///
/// # Errors
/// [`SuggestionError::UnknownChange`] for an empty choice, or a place past the last change.
fn checked_choice(
    chosen: Option<Vec<usize>>,
    count: usize,
) -> Result<Option<Vec<usize>>, SuggestionError> {
    let Some(mut chosen) = chosen else {
        return Ok(None);
    };
    if chosen.is_empty() {
        return Err(SuggestionError::UnknownChange(
            "choose at least one change to accept, or dismiss the suggestion".to_owned(),
        ));
    }
    if let Some(unknown) = chosen.iter().find(|index| **index >= count) {
        return Err(SuggestionError::UnknownChange(format!(
            "the suggestion has no change {unknown}; it has {count}, numbered from 0"
        )));
    }
    chosen.sort_unstable();
    chosen.dedup();
    if chosen.len() == count {
        return Ok(None);
    }
    Ok(Some(chosen))
}

/// `current` with `edits` applied in order. Each edit's `find` must be in the text exactly once as
/// it stands after the edits before it, so an edit changes only what its author saw; an empty
/// `find` adds `replace` as a line of its own at the end. The result's line endings and trailing
/// whitespace are normalized as a publish normalizes a page's.
///
/// # Errors
/// [`SuggestionError::EditDoesNotApply`] naming the edit, by its place from 1, whose text is missing
/// or found more than once.
pub fn apply_edits(current: &str, edits: &[Edit]) -> Result<String, SuggestionError> {
    let mut text = current.to_owned();
    for (index, edit) in edits.iter().enumerate() {
        let number = index + 1;
        if edit.find.is_empty() {
            let addition = edit.replace.trim();
            if !addition.is_empty() {
                if !text.trim().is_empty() {
                    text.push('\n');
                }
                text.push_str(addition);
            }
            continue;
        }
        match text.match_indices(&edit.find).count() {
            1 => text = text.replacen(&edit.find, &edit.replace, 1),
            0 => {
                return Err(SuggestionError::EditDoesNotApply(format!(
                    "edit {number}'s find text is not in the page; read it again and copy the \
                     text exactly"
                )));
            }
            _ => {
                return Err(SuggestionError::EditDoesNotApply(format!(
                    "edit {number}'s find text is in the page more than once; include more of the \
                     text around it"
                )));
            }
        }
    }
    Ok(text.replace("\r\n", "\n").trim_end().to_owned())
}

/// One change a suggestion makes, as a person reviews it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// Where its first changed line is in the version it was written from, from 1. A change that
    /// only adds lines names the line they come before, or one past the last.
    pub line: usize,
    /// The lines it changes and those around it, before and after it.
    pub before: String,
    pub after: String,
}

/// A run of changed lines: where it sits in each text, by line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Span {
    old_start: usize,
    old_end: usize,
    new_start: usize,
    new_end: usize,
}

/// `text`'s lines, exactly: an empty text has none, and joining them with `\n` gives it back.
fn lines_of(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.split('\n').collect()
}

/// Every run of lines `body` changes against `base`, in order. Runs of deleted, inserted, and
/// replaced lines that touch are one change, since a reviewer reads them as one.
fn spans(base: &[&str], body: &[&str]) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for operation in similar::capture_diff_slices(Algorithm::Myers, base, body) {
        if let DiffOp::Equal { .. } = operation {
            continue;
        }
        let old = operation.old_range();
        let new = operation.new_range();
        match spans.last_mut() {
            Some(last) if last.old_end == old.start && last.new_end == new.start => {
                last.old_end = old.end;
                last.new_end = new.end;
            }
            _ => spans.push(Span {
                old_start: old.start,
                old_end: old.end,
                new_start: new.start,
                new_end: new.end,
            }),
        }
    }
    spans
}

/// The changes `body` makes to `base`, in order, each with [`CONTEXT_LINES`] unchanged lines on
/// either side.
pub fn changes(base: &str, body: &str) -> Vec<Change> {
    let base_lines = lines_of(base);
    let body_lines = lines_of(body);
    spans(&base_lines, &body_lines)
        .into_iter()
        .map(|span| {
            let lead = span.old_start.saturating_sub(CONTEXT_LINES);
            let trail = (span.old_end + CONTEXT_LINES).min(base_lines.len());
            let before = base_lines[lead..trail].join("\n");
            let after = [
                &base_lines[lead..span.old_start],
                &body_lines[span.new_start..span.new_end],
                &base_lines[span.old_end..trail],
            ]
            .concat()
            .join("\n");
            Change {
                line: span.old_start + 1,
                before,
                after,
            }
        })
        .collect()
}

/// `base` with the changes `body` makes to it, or only those `chosen` names by their place among
/// [`changes`]; every other line as `base` has it.
fn take(base: &str, body: &str, chosen: Option<&[usize]>) -> String {
    let base_lines = lines_of(base);
    let body_lines = lines_of(body);
    let mut taken: Vec<&str> = Vec::with_capacity(body_lines.len());
    let mut next_old = 0;
    for (index, span) in spans(&base_lines, &body_lines).into_iter().enumerate() {
        taken.extend_from_slice(&base_lines[next_old..span.old_start]);
        if chosen.is_none_or(|chosen| chosen.contains(&index)) {
            taken.extend_from_slice(&body_lines[span.new_start..span.new_end]);
        } else {
            taken.extend_from_slice(&base_lines[span.old_start..span.old_end]);
        }
        next_old = span.old_end;
    }
    taken.extend_from_slice(&base_lines[next_old..]);
    taken.join("\n")
}

/// The page with both changes: `current` (what it holds now) and `proposed`, each made from `base`,
/// merged line by line. `None` when the two change the same lines differently.
fn merged(base: &str, current: &str, proposed: &str) -> Option<String> {
    if current == base {
        return Some(proposed.to_owned());
    }
    // diffy merges whole lines, so each text ends with its last line's newline; the result's is
    // dropped again, as a page is stored without one.
    let with_newline = |text: &str| format!("{text}\n");
    diffy::merge(
        &with_newline(base),
        &with_newline(current),
        &with_newline(proposed),
    )
    .ok()
    .map(|merged| merged.trim_end().to_owned())
}

/// Whether each of `suggestion`'s changes would still merge, alone, with a page that holds
/// `current`: all of them while the page is as the suggestion found it.
pub fn applies(suggestion: &Suggestion, current: &str) -> Vec<bool> {
    let count = changes(&suggestion.base_body, &suggestion.body).len();
    if current == suggestion.base_body {
        return vec![true; count];
    }
    (0..count)
        .map(|index| {
            let taken = take(&suggestion.base_body, &suggestion.body, Some(&[index]));
            merged(&suggestion.base_body, current, &taken).is_some()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(find: &str, replace: &str) -> Edit {
        Edit {
            find: find.to_owned(),
            replace: replace.to_owned(),
        }
    }

    #[test]
    fn edits_replace_text_found_once_and_add_to_the_end_when_find_is_empty() {
        let people = "- Priya Raman, head of operations\n- Tom Okafor, sales";

        let renamed = apply_edits(people, &[edit("Tom Okafor, sales", "Tomás Okafor, sales")])
            .expect("applies");
        assert_eq!(
            renamed,
            "- Priya Raman, head of operations\n- Tomás Okafor, sales"
        );

        let added = apply_edits(people, &[edit("", "- Lena Brandt, finance\n")]).expect("adds");
        assert_eq!(added, format!("{people}\n- Lena Brandt, finance"));

        let first_entry = apply_edits("", &[edit("", "- Lena Brandt")]).expect("adds");
        assert_eq!(
            first_entry, "- Lena Brandt",
            "an empty page starts with no blank line"
        );
    }

    #[test]
    fn an_edit_whose_text_is_missing_or_repeated_is_refused_naming_it() {
        let glossary = "- ACME: a client\n- ACME: a client";
        let repeated = apply_edits(glossary, &[edit("ACME: a client", "ACME Corp: a client")])
            .expect_err("ambiguous");
        assert!(
            repeated.to_string().contains("more than once"),
            "{repeated}"
        );

        let missing = apply_edits("- ACME", &[edit("", "- Beta"), edit("Gamma", "Delta")])
            .expect_err("missing");
        assert!(missing.to_string().starts_with("edit 2's"), "{missing}");
    }

    #[test]
    fn a_suggestion_is_checked_and_trimmed_before_it_is_stored() {
        let page = Uuid::now_v7();
        let new = NewSuggestion::new(
            page,
            Some(3),
            vec![edit("", "- ACME")],
            "  Said in the kickoff.  ",
            Some(" "),
            Some(" Kickoff, November 3rd "),
        )
        .expect("checks");
        assert_eq!(new.reason, "Said in the kickoff.");
        assert_eq!(new.quote, None, "a blank quote is no quote");
        assert_eq!(new.source.as_deref(), Some("Kickoff, November 3rd"));
        assert_eq!(new.base_version, Some(3));

        let no_edits =
            NewSuggestion::new(page, None, Vec::new(), "Why", None, None).expect_err("refused");
        assert!(
            no_edits.to_string().contains("at least one edit"),
            "{no_edits}"
        );

        let blank_reason = NewSuggestion::new(page, None, vec![edit("", "x")], "  ", None, None)
            .expect_err("refused");
        assert!(
            blank_reason.to_string().contains("reason"),
            "{blank_reason}"
        );

        let many = vec![edit("", "x"); EDITS_MAX + 1];
        let too_many =
            NewSuggestion::new(page, None, many, "Why", None, None).expect_err("refused");
        assert!(too_many.to_string().contains("at most 20"), "{too_many}");
    }

    #[test]
    fn a_suggestion_reads_as_each_run_of_lines_it_changes_with_lines_around_it() {
        let base = "# Glossary\n\n- ACME: a client\n- Beta: a product\n- Gamma\n- Kilo\n- Tango";
        let body = "# Glossary\n\n- ACME Corp: a client\n- Beta: a product\n- Gamma\n- Kilo\n- Sierra\n- Tango";

        let found = changes(base, body);

        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].line, 3);
        assert_eq!(
            found[0].before,
            "# Glossary\n\n- ACME: a client\n- Beta: a product\n- Gamma"
        );
        assert_eq!(
            found[0].after,
            "# Glossary\n\n- ACME Corp: a client\n- Beta: a product\n- Gamma"
        );
        assert_eq!(
            found[1].line, 7,
            "an added line names the line it comes before"
        );
        assert_eq!(found[1].before, "- Gamma\n- Kilo\n- Tango");
        assert_eq!(found[1].after, "- Gamma\n- Kilo\n- Sierra\n- Tango");
    }

    #[test]
    fn a_page_that_was_empty_reads_as_one_change_with_nothing_before_it() {
        let found = changes("", "- ACME");
        assert_eq!(
            found,
            [Change {
                line: 1,
                before: String::new(),
                after: "- ACME".to_owned(),
            }]
        );
    }

    #[test]
    fn taking_some_changes_leaves_every_other_line_as_the_version_had_it() {
        let base = "- Alpha\n- Delta\n- Kilo\n- Tango";
        let body = "- Alpha\n- Bravo\n- Delta\n- Kilo\n- Sierra\n- Tango";

        assert_eq!(take(base, body, None), body, "every change");
        assert_eq!(
            take(base, body, Some(&[1])),
            "- Alpha\n- Delta\n- Kilo\n- Sierra\n- Tango"
        );
        assert_eq!(
            take(base, body, Some(&[0])),
            "- Alpha\n- Bravo\n- Delta\n- Kilo\n- Tango"
        );
        assert_eq!(take("", "- ACME", Some(&[0])), "- ACME");
        assert_eq!(take("- Gone\n- Kept", "- Kept", Some(&[0])), "- Kept");
    }

    #[test]
    fn a_choice_of_every_change_is_recorded_as_all_of_them() {
        assert_eq!(checked_choice(None, 3).expect("all"), None);
        assert_eq!(
            checked_choice(Some(vec![2, 0, 1, 0]), 3).expect("all"),
            None
        );
        assert_eq!(
            checked_choice(Some(vec![2, 0, 2]), 3).expect("some"),
            Some(vec![0, 2])
        );
        checked_choice(Some(Vec::new()), 3).expect_err("none is no choice");
        let unknown = checked_choice(Some(vec![3]), 3).expect_err("past the last");
        assert!(unknown.to_string().contains("no change 3"), "{unknown}");
    }

    #[test]
    fn accepting_after_the_page_changed_merges_both_changes() {
        let base = "- Alpha\n- Delta\n- Kilo\n- Tango";
        // Someone added Bravo since; the suggestion adds Sierra further down.
        let current = "- Alpha\n- Bravo\n- Delta\n- Kilo\n- Tango";
        let proposed = "- Alpha\n- Delta\n- Kilo\n- Sierra\n- Tango";

        assert_eq!(
            merged(base, current, proposed).as_deref(),
            Some("- Alpha\n- Bravo\n- Delta\n- Kilo\n- Sierra\n- Tango")
        );
        assert_eq!(
            merged(base, base, proposed).as_deref(),
            Some(proposed),
            "an unchanged page takes the suggestion as it is"
        );
    }

    #[test]
    fn accepting_a_change_to_lines_changed_since_is_refused() {
        let base = "- Priya Ramen, operations";
        let current = "- Priya Raman, operations";
        let proposed = "- Priya Rahman, operations";

        assert_eq!(merged(base, current, proposed), None);
    }

    #[test]
    fn each_change_says_whether_it_still_merges_with_the_page_as_it_reads_now() {
        let base = "- Priya Ramen, operations\n- Tom\n- Uma\n- Vik\n- Wes";
        let body = "- Priya Rahman, operations\n- Tom\n- Uma\n- Vik\n- Wes\n- Xia";
        let current = "- Priya Raman, operations\n- Tom\n- Uma\n- Vik\n- Wes";
        let suggestion = Suggestion {
            id: Uuid::now_v7(),
            page_id: Uuid::now_v7(),
            base_version: 1,
            base_body: base.to_owned(),
            body: body.to_owned(),
            reason: "Heard it".to_owned(),
            quote: None,
            source: None,
            timeline_record_id: None,
            workflow_run_id: None,
            state: "pending".to_owned(),
            accepted_changes: None,
            dismissal_reason: None,
            decided_by: None,
            decided_at: None,
            created_by: Uuid::now_v7(),
            agent: None,
            agent_name: None,
            created_at: Utc::now(),
        };

        assert_eq!(
            applies(&suggestion, current),
            [false, true],
            "the name was corrected since, the new line still lands"
        );
        assert_eq!(applies(&suggestion, base), [true, true]);
    }
}
