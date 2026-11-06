// Copyright © 2026 Jalapeno Labs

//! Learnings: the pages that hold a category and evidence, and the rules they keep over a page.
//!
//! A learning is written for the next agent: a takeaway title, a body that leads with the rule, the
//! key of the category it is filed under (`crate::models::learning_category`), and its evidence
//! (`docs/knowledge.md`, Learnings). It is a page (`crate::models::page`), created under the Learnings
//! page, so its revisions are versions, its history is the page's, and it is in the tree and the
//! index as every page is. What this module adds is what a learning means: a title names one, whatever
//! its case, so [`save`] revises the learning that has it and creates one only when none does, and any
//! change can be undone field by field ([`undo`]).
//!
//! History keeps its shape here, whatever a page stores: a [`HistoryEntry`] names the fields each
//! revision changed, read from the versions either side of it, and a page's moves are not part of it.
//!
//! Deleting is soft: a deleted learning keeps its page and history and can be restored.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::{page_events, page_versions, pages};
use crate::models::agent::Author;
use crate::models::page::{
    self, LEARNINGS_PAGE_ID, LearningFields, Page, PageContent, PageEdit, PageError, PageVersion,
    Provenance,
};

diesel::define_sql_function! {
    /// Postgres's `lower`, by which `pages_learning_title_unique` makes a title name one learning.
    fn lower(text: diesel::sql_types::Text) -> diesel::sql_types::Text;
}

/// What a learning says: everything a save writes and a revision changes.
///
/// Serialized with its fields' own names, which are the field names history records changes
/// under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LearningContent {
    pub title: String,
    pub body: String,
    /// The key of the category it is filed under.
    pub category: String,
    pub evidence: String,
}

impl From<LearningContent> for PageContent {
    fn from(content: LearningContent) -> Self {
        Self {
            title: content.title,
            body: content.body,
            category: Some(content.category),
            evidence: Some(content.evidence),
        }
    }
}

impl From<PageContent> for LearningContent {
    /// Only a learning's content converts: no page becomes a learning, so each of a learning's
    /// versions holds both.
    fn from(content: PageContent) -> Self {
        Self {
            title: content.title,
            body: content.body,
            category: content
                .category
                .expect("a learning's every version has a category"),
            evidence: content
                .evidence
                .expect("a learning's every version has evidence"),
        }
    }
}

/// A stored learning.
#[derive(Debug, Clone)]
pub struct Learning {
    pub id: Uuid,
    pub title: String,
    pub body: String,
    /// The key of the category it is filed under.
    pub category: String,
    pub evidence: String,
    /// The agent that acted for `created_by` when it was first saved, in its recorded form.
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub deleted_at: Option<DateTime<Utc>>,
    /// Who first saved it.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Learning {
    /// The learning `page` holds, first saved through `agent`.
    fn of(page: Page, agent: Option<String>, agent_name: Option<String>) -> Self {
        Self {
            id: page.id,
            title: page.title,
            body: page.body,
            category: page.category.expect("a learning has a category"),
            evidence: page.evidence.expect("a learning has evidence"),
            agent,
            agent_name,
            deleted_at: page.deleted_at,
            created_by: page.created_by,
            created_at: page.created_at,
            updated_at: page.updated_at,
        }
    }
}

/// What a history entry records, and so how its `data` is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// The learning was first saved. `data` is its content as saved.
    Created,
    /// Its content changed. `data.changes` maps each field to `{ from, to }`, `data.undid` names
    /// the entry an undo reversed, and `data.restoredFrom` the version a restore copied.
    Revised,
    /// `data` is empty, or `{ undid }`.
    Deleted,
    /// `data` is `{ deletedAt }`, the deletion it reversed, and `undid` for an undo.
    Restored,
}

impl HistoryKind {
    /// The name sent to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Revised => "revised",
            Self::Deleted => "deleted",
            Self::Restored => "restored",
        }
    }
}

/// A change to a learning, as it reads in the learning's history.
#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub id: Uuid,
    pub learning_id: Uuid,
    /// A [`HistoryKind`] name.
    pub kind: String,
    /// Who made the change.
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

/// A learning as a write left it, with the history entries the write recorded.
#[derive(Debug)]
pub struct Recorded {
    pub learning: Learning,
    /// Oldest first; empty when the write turned out to change nothing.
    pub history: Vec<HistoryEntry>,
    /// Every page clients must load again, as [`page::Recorded`] lists them.
    pub changed: Vec<Uuid>,
    /// Those of them that are learnings.
    pub learnings: Vec<Uuid>,
}

/// A correction: each field given replaces the learning's.
#[derive(Debug, Default)]
pub struct LearningChanges {
    pub title: Option<String>,
    pub body: Option<String>,
    pub category: Option<String>,
    pub evidence: Option<String>,
}

/// Why a write to a learning was refused.
#[derive(Debug, thiserror::Error)]
pub enum LearningError {
    /// Includes [`DieselError::NotFound`] for an unknown learning or history entry.
    #[error(transparent)]
    Database(#[from] DieselError),
    #[error("another learning already has this title")]
    TitleTaken,
    #[error("this learning is deleted; restore it to change it")]
    Deleted,
    /// No category has the key a write named. `known` is every key there is, in the order they
    /// are offered, so the agent or person who named it can choose again.
    #[error("`{key}` is not a learning category; use one of: {}", known.join(", "))]
    UnknownCategory { key: String, known: Vec<String> },
    /// The change an undo names cannot be reversed as the learning stands; the message says why.
    #[error("{0}")]
    NothingToUndo(&'static str),
    /// Its page refused the write, such as bringing it back under a deleted page.
    #[error(transparent)]
    Page(PageError),
}

impl From<PageError> for LearningError {
    fn from(error: PageError) -> Self {
        match error {
            PageError::Database(error) => Self::Database(error),
            PageError::TitleTaken => Self::TitleTaken,
            PageError::Deleted => Self::Deleted,
            PageError::UnknownCategory { key, known } => Self::UnknownCategory { key, known },
            other => Self::Page(other),
        }
    }
}

/// Saves `content`: revises the learning with its title, bringing it back if it was deleted, or
/// creates one when no learning has it.
///
/// # Errors
/// [`LearningError::UnknownCategory`] for a category that does not exist, and any database error.
pub async fn save(
    connection: &mut AsyncPgConnection,
    content: LearningContent,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    match save_once(connection, content.clone(), author).await {
        // Two saves of a new title at once: the second insert meets the first's row, so saving
        // again revises it.
        Err(LearningError::TitleTaken) => save_once(connection, content, author).await,
        outcome => outcome,
    }
}

async fn save_once(
    connection: &mut AsyncPgConnection,
    content: LearningContent,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    connection
        .transaction(async move |connection| {
            // Read unlocked: bringing a learning back takes the tree's lock, which every change to
            // the tree takes before any page's, so no page's lock may be held by then.
            let existing = pages::table
                .filter(pages::category.is_not_null())
                .filter(lower(pages::title).eq(lower(&content.title)))
                .select(Page::as_select())
                .first(connection)
                .await
                .optional()?;
            let Some(existing) = existing else {
                return create(connection, content, author).await;
            };

            let mut written = Vec::new();
            if existing.deleted_at.is_some() {
                written.push(page::restore(connection, existing.id, author, None).await?);
            }
            let locked = page::lock(connection, existing.id).await?;
            if locked.deleted_at.is_some() {
                return Err(LearningError::Deleted);
            }
            let content = PageContent::from(content);
            if content != locked.content() {
                let provenance = Provenance::default();
                written.push(
                    page::publish_content(connection, &locked, content, provenance, author).await?,
                );
            }
            Ok(recorded(connection, locked, written).await?)
        })
        .await
}

async fn create(
    connection: &mut AsyncPgConnection,
    content: LearningContent,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    let edit = PageEdit {
        title: content.title,
        body: content.body,
        learning: Some(LearningFields {
            category: content.category,
            evidence: content.evidence,
        }),
    };
    let created = page::create(connection, Some(LEARNINGS_PAGE_ID), edit, author).await?;
    let page = created.page.clone();
    Ok(recorded(connection, page, vec![created]).await?)
}

/// Corrects learning `id` with `changes`.
///
/// # Errors
/// `NotFound` for an unknown learning, [`LearningError::Deleted`] for a deleted one, and
/// [`LearningError::TitleTaken`] when another learning has the new title.
pub async fn correct(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    changes: LearningChanges,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    connection
        .transaction(async move |connection| {
            let locked = page::lock(connection, id).await?;
            if !locked.is_learning() {
                return Err(DieselError::NotFound.into());
            }
            if locked.deleted_at.is_some() {
                return Err(LearningError::Deleted);
            }
            let current = LearningContent::from(locked.content());
            let corrected = PageContent::from(LearningContent {
                title: changes.title.unwrap_or(current.title),
                body: changes.body.unwrap_or(current.body),
                category: changes.category.unwrap_or(current.category),
                evidence: changes.evidence.unwrap_or(current.evidence),
            });
            if corrected == locked.content() {
                return Ok(recorded(connection, locked, Vec::new()).await?);
            }
            let provenance = Provenance::default();
            let revised =
                page::publish_content(connection, &locked, corrected, provenance, author).await?;
            Ok(recorded(connection, locked, vec![revised]).await?)
        })
        .await
}

/// Deletes learning `id` softly, taking it out of the index. Deleting a deleted learning
/// changes nothing.
///
/// # Errors
/// `NotFound` for an unknown learning.
pub async fn delete(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    connection
        .transaction(async move |connection| {
            let learning = find_page(connection, id).await?;
            let deleted = page::delete(connection, id, author, None).await?;
            Ok(recorded(connection, learning, vec![deleted]).await?)
        })
        .await
}

/// Brings deleted learning `id` back, into the index again. Restoring a live learning changes
/// nothing.
///
/// # Errors
/// `NotFound` for an unknown learning, and [`LearningError::Page`] while the page it sits under is
/// deleted.
pub async fn restore(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    connection
        .transaction(async move |connection| {
            let learning = find_page(connection, id).await?;
            let restored = page::restore(connection, id, author, None).await?;
            Ok(recorded(connection, learning, vec![restored]).await?)
        })
        .await
}

/// Reverses history entry `entry_id` of learning `id`, recording the reversal as a change of its
/// own that names the entry.
///
/// A revision is undone field by field ([`page::undo_version`]): each field that still holds
/// what it wrote gets back what it replaced, and a field changed again since keeps its newer value.
/// Undoing the first save or a restore deletes the learning, and undoing a delete restores it.
///
/// # Errors
/// `NotFound` for an unknown learning or an entry of another learning,
/// [`LearningError::NothingToUndo`] when the change no longer shows in the learning (it was
/// undone already, or every field it wrote has changed since), [`LearningError::Deleted`] for
/// a revision of a deleted learning, and [`LearningError::TitleTaken`] when the title it would
/// put back is another learning's now.
pub async fn undo(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    entry_id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    use page::HistoryKind::{
        Attached, Created, Deleted, Detached, Labeled, Moved, Published, Restored, Unlabeled,
    };

    connection
        .transaction(async move |connection| {
            let learning = find_page(connection, id).await?;
            let entry = page::history_entry(connection, id, entry_id).await?;
            let undid = Some(entry.id);
            let is_deleted = learning.deleted_at.is_some();

            let written = match page::HistoryKind::parse(&entry.kind) {
                Some(Created | Restored) if is_deleted => {
                    return Err(LearningError::NothingToUndo(
                        "the learning is already deleted",
                    ));
                }
                Some(Created | Restored) => page::delete(connection, id, author, undid).await?,
                Some(Deleted) if !is_deleted => {
                    return Err(LearningError::NothingToUndo("the learning is not deleted"));
                }
                Some(Deleted) => page::restore(connection, id, author, undid).await?,
                Some(Published) if is_deleted => return Err(LearningError::Deleted),
                Some(Published) => undo_revision(connection, id, &entry, author).await?,
                Some(Moved | Labeled | Unlabeled | Attached | Detached) | None => {
                    return Err(LearningError::NothingToUndo(
                        "this kind of change cannot be undone",
                    ));
                }
            };
            Ok(recorded(connection, learning, vec![written]).await?)
        })
        .await
}

/// Reverses the revision `entry` records, in the transaction of [`undo`].
async fn undo_revision(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    entry: &page::HistoryEntry,
    author: Author<'_>,
) -> Result<page::Recorded, LearningError> {
    let version = entry
        .published_version()
        .ok_or(LearningError::NothingToUndo(
            "this change's record cannot be read",
        ))?;
    let locked = page::lock(connection, id).await?;
    let provenance = Provenance {
        undid: Some(entry.id),
        ..Provenance::default()
    };
    page::undo_version(connection, &locked, version, provenance, author)
        .await
        .map_err(|error| match error {
            PageError::UnknownCategory { .. } => LearningError::NothingToUndo(
                "the category this change would put back has been removed",
            ),
            other => other.into(),
        })?
        .ok_or(LearningError::NothingToUndo(
            "every field this change wrote has changed again since",
        ))
}

/// Moves every learning filed under category `from`, deleted ones included, to category `to`,
/// each move a revision in its history by `author`. Part of the caller's transaction, which
/// must already hold `to` ([`crate::models::learning_category::hold`]), as removing `from` does.
///
/// # Errors
/// Propagates any database error.
pub async fn refile(
    connection: &mut AsyncPgConnection,
    from: &str,
    to: &str,
    author: Author<'_>,
) -> Result<Vec<Recorded>, LearningError> {
    let filed: Vec<Page> = pages::table
        .filter(pages::category.eq(from))
        .order(pages::id.asc())
        .for_update()
        .select(Page::as_select())
        .load(connection)
        .await?;
    let mut moved = Vec::with_capacity(filed.len());
    for learning in filed {
        let content = PageContent {
            category: Some(to.to_owned()),
            ..learning.content()
        };
        let provenance = Provenance::default();
        let revised =
            page::publish_content(connection, &learning, content, provenance, author).await?;
        moved.push(recorded(connection, learning, vec![revised]).await?);
    }
    Ok(moved)
}

/// The learning `page` holds after `written`, the writes made to it in order, each of which left a
/// newer page than `page`; with none, as `page` stands.
async fn recorded(
    connection: &mut AsyncPgConnection,
    page: Page,
    written: Vec<page::Recorded>,
) -> QueryResult<Recorded> {
    let mut newest = page;
    let mut stored = Vec::new();
    let mut changed = Vec::new();
    let mut learnings = Vec::new();
    for write in written {
        newest = write.page;
        stored.extend(write.history);
        changed.extend(write.changed);
        learnings.extend(write.learnings);
    }
    let first = page::version_summary(connection, newest.id, 1).await?;
    Ok(Recorded {
        learning: Learning::of(newest, first.agent, first.agent_name),
        history: entries(connection, stored).await?,
        changed,
        learnings,
    })
}

/// `stored` as a learning's history reads: each revision naming the fields it changed, from the
/// versions either side of it, and each first save what it said.
async fn entries(
    connection: &mut AsyncPgConnection,
    stored: Vec<page::HistoryEntry>,
) -> QueryResult<Vec<HistoryEntry>> {
    let mut learnings = Vec::with_capacity(stored.len());
    let mut wanted = vec![1];
    for entry in &stored {
        learnings.push(entry.page_id);
        if let Some(version) = entry.published_version() {
            wanted.extend([version - 1, version]);
        }
    }
    let versions: Vec<PageVersion> = page_versions::table
        .filter(page_versions::page_id.eq_any(&learnings))
        .filter(page_versions::version.eq_any(&wanted))
        .select(PageVersion::as_select())
        .load(connection)
        .await?;
    let versions: HashMap<(Uuid, i32), LearningContent> = versions
        .into_iter()
        .map(|version| {
            let key = (version.summary.page_id, version.summary.version);
            (key, LearningContent::from(version.into_content()))
        })
        .collect();
    Ok(stored
        .into_iter()
        .filter_map(|entry| entry_of(entry, &versions))
        .collect())
}

/// `entry` as a learning's history reads it, `None` for what its history does not hold. `versions`
/// holds the versions it names.
fn entry_of(
    entry: page::HistoryEntry,
    versions: &HashMap<(Uuid, i32), LearningContent>,
) -> Option<HistoryEntry> {
    let content_of = |version: i32| {
        let content = versions.get(&(entry.page_id, version));
        if content.is_none() {
            // Versions are never removed, so a page cannot come to this on its own.
            event!(
                name: "learnings.history.version_missing",
                Level::WARN,
                page.id = %entry.page_id,
                page.version = version,
                "a history entry names a version the learning does not have; leaving it out",
            );
        }
        content
    };
    let (kind, data) = match page::HistoryKind::parse(&entry.kind)? {
        page::HistoryKind::Created => {
            let data = serde_json::to_value(content_of(1)?).expect("learning content serializes");
            (HistoryKind::Created, data)
        }
        page::HistoryKind::Published => {
            let version = entry.published_version()?;
            let changes = changes_between(content_of(version - 1)?, content_of(version)?);
            let mut data = json!({ "changes": changes });
            for carried in ["undid", "restoredFrom"] {
                if let Some(value) = entry.data.get(carried) {
                    data[carried] = value.clone();
                }
            }
            (HistoryKind::Revised, data)
        }
        page::HistoryKind::Deleted => (HistoryKind::Deleted, entry.data.clone()),
        page::HistoryKind::Restored => (HistoryKind::Restored, entry.data.clone()),
        page::HistoryKind::Moved
        | page::HistoryKind::Labeled
        | page::HistoryKind::Unlabeled
        | page::HistoryKind::Attached
        | page::HistoryKind::Detached => {
            return None;
        }
    };
    Some(HistoryEntry {
        id: entry.id,
        learning_id: entry.page_id,
        kind: kind.as_str().to_owned(),
        created_by: entry.created_by,
        agent: entry.agent,
        agent_name: entry.agent_name,
        data,
        created_at: entry.created_at,
    })
}

/// Each field whose value differs between `before` and `after`, as `{ from, to }`.
fn changes_between(before: &LearningContent, after: &LearningContent) -> Map<String, Value> {
    let (Value::Object(before), Value::Object(after)) = (
        serde_json::to_value(before).expect("learning content serializes"),
        serde_json::to_value(after).expect("learning content serializes"),
    ) else {
        unreachable!("a struct serializes as an object");
    };
    let mut changes = Map::new();
    for (field, from) in before {
        let to = &after[&field];
        if &from != to {
            changes.insert(field, json!({ "from": from, "to": to }));
        }
    }
    changes
}

/// The page learning `id` holds, live or deleted.
async fn find_page(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Page> {
    pages::table
        .find(id)
        .filter(pages::category.is_not_null())
        .select(Page::as_select())
        .first(connection)
        .await
}

/// Learning `id`, live or deleted.
///
/// # Errors
/// `NotFound` for an unknown learning.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Learning> {
    let page = find_page(connection, id).await?;
    let first = page::version_summary(connection, id, 1).await?;
    Ok(Learning::of(page, first.agent, first.agent_name))
}

/// Live learnings, most recently changed first; or deleted ones, most recently deleted first.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection, deleted: bool) -> QueryResult<Vec<Learning>> {
    let query = pages::table
        .inner_join(
            page_versions::table.on(page_versions::page_id
                .eq(pages::id)
                .and(page_versions::version.eq(1))),
        )
        .filter(pages::category.is_not_null())
        .select((
            Page::as_select(),
            page_versions::agent,
            page_versions::agent_name,
        ));
    let rows: Vec<(Page, Option<String>, Option<String>)> = if deleted {
        query
            .filter(pages::deleted_at.is_not_null())
            .order(pages::deleted_at.desc())
            .load(connection)
            .await?
    } else {
        query
            .filter(pages::deleted_at.is_null())
            .order(pages::updated_at.desc())
            .load(connection)
            .await?
    };
    Ok(rows
        .into_iter()
        .map(|(page, agent, agent_name)| Learning::of(page, agent, agent_name))
        .collect())
}

/// Learning `id`'s history, oldest first.
///
/// # Errors
/// `NotFound` for an unknown learning.
pub async fn history(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Vec<HistoryEntry>> {
    find_page(connection, id).await?;
    let stored = page::history(connection, id).await?;
    entries(connection, stored).await
}

/// Every learning's changes, newest first, each with its learning as it stands: at most
/// `limit`, and only those written before entry `before` when it is given, to read on.
///
/// # Errors
/// Propagates any database error.
pub async fn feed(
    connection: &mut AsyncPgConnection,
    before: Option<Uuid>,
    limit: i64,
) -> QueryResult<Vec<(HistoryEntry, Learning)>> {
    // Moves, labels, and files are a page's alone, and a limit counts only what the feed shows.
    let mut query = page_events::table
        .inner_join(pages::table)
        .filter(pages::category.is_not_null())
        .filter(page_events::kind.ne_all([
            page::HistoryKind::Moved.as_str(),
            page::HistoryKind::Labeled.as_str(),
            page::HistoryKind::Unlabeled.as_str(),
            page::HistoryKind::Attached.as_str(),
            page::HistoryKind::Detached.as_str(),
        ]))
        .select((page::HistoryEntry::as_select(), Page::as_select()))
        .order(page_events::id.desc())
        .limit(limit)
        .into_boxed();
    if let Some(before) = before {
        query = query.filter(page_events::id.lt(before));
    }
    let rows: Vec<(page::HistoryEntry, Page)> = query.load(connection).await?;

    let ids: Vec<Uuid> = rows.iter().map(|(_, page)| page.id).collect();
    let first_saves: Vec<(Uuid, Option<String>, Option<String>)> = page_versions::table
        .filter(page_versions::page_id.eq_any(&ids))
        .filter(page_versions::version.eq(1))
        .select((
            page_versions::page_id,
            page_versions::agent,
            page_versions::agent_name,
        ))
        .load(connection)
        .await?;
    let first_saves: HashMap<Uuid, (Option<String>, Option<String>)> = first_saves
        .into_iter()
        .map(|(id, agent, agent_name)| (id, (agent, agent_name)))
        .collect();

    let (stored, learning_pages): (Vec<_>, Vec<_>) = rows.into_iter().unzip();
    let learnings: HashMap<Uuid, Learning> = learning_pages
        .into_iter()
        .map(|page| {
            let (agent, agent_name) = first_saves.get(&page.id).cloned().unwrap_or_default();
            (page.id, Learning::of(page, agent, agent_name))
        })
        .collect();
    let feed = entries(connection, stored)
        .await?
        .into_iter()
        .filter_map(|entry| {
            let learning = learnings.get(&entry.learning_id)?.clone();
            Some((entry, learning))
        })
        .collect();
    Ok(feed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> LearningContent {
        LearningContent {
            title: "Rebuild indexes after a collation change".to_owned(),
            body: "Run REINDEX DATABASE.".to_owned(),
            category: "procedure".to_owned(),
            evidence: "Lookups missed rows after moving to glibc.".to_owned(),
        }
    }

    fn stored(kind: &str, data: Value) -> page::HistoryEntry {
        page::HistoryEntry {
            id: Uuid::now_v7(),
            page_id: Uuid::nil(),
            kind: kind.to_owned(),
            created_by: Uuid::nil(),
            agent: None,
            agent_name: None,
            data,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn naming_an_unknown_category_says_which_ones_there_are() {
        let error = LearningError::UnknownCategory {
            key: "gotcha".to_owned(),
            known: vec!["decision".to_owned(), "rtl".to_owned()],
        };
        assert_eq!(
            error.to_string(),
            "`gotcha` is not a learning category; use one of: decision, rtl"
        );
    }

    #[test]
    fn a_revision_records_each_changed_field_from_and_to() {
        let before = content();
        let after = LearningContent {
            body: "Run REINDEX DATABASE, then record the new version.".to_owned(),
            category: "pitfall".to_owned(),
            ..content()
        };

        let changes = changes_between(&before, &after);

        assert_eq!(
            Value::Object(changes),
            json!({
                "body": {
                    "from": "Run REINDEX DATABASE.",
                    "to": "Run REINDEX DATABASE, then record the new version.",
                },
                "category": { "from": "procedure", "to": "pitfall" },
            })
        );
        assert!(changes_between(&before, &content()).is_empty());
    }

    #[test]
    fn a_version_reads_as_the_revision_it_was() {
        let revised = LearningContent {
            body: "Revised.".to_owned(),
            ..content()
        };
        let versions = HashMap::from([((Uuid::nil(), 1), content()), ((Uuid::nil(), 2), revised)]);

        let undid = Uuid::now_v7();
        let entry = entry_of(
            stored("published", json!({ "version": 2, "undid": undid })),
            &versions,
        )
        .expect("part of its history");

        assert_eq!(entry.kind, "revised");
        assert_eq!(
            entry.data,
            json!({
                "changes": { "body": { "from": "Run REINDEX DATABASE.", "to": "Revised." } },
                "undid": undid,
            })
        );
    }

    #[test]
    fn a_first_save_reads_as_what_it_said() {
        let versions = HashMap::from([((Uuid::nil(), 1), content())]);
        let entry = entry_of(
            stored(
                "created",
                json!({ "parentId": LEARNINGS_PAGE_ID, "version": 1 }),
            ),
            &versions,
        )
        .expect("part of its history");

        assert_eq!(entry.kind, "created");
        assert_eq!(
            entry.data,
            serde_json::to_value(content()).expect("serializes")
        );
    }

    #[test]
    fn a_move_and_a_kind_this_build_does_not_know_are_no_part_of_its_history() {
        let versions = HashMap::new();
        assert!(entry_of(stored("moved", json!({})), &versions).is_none());
        assert!(entry_of(stored("archived", json!({})), &versions).is_none());
    }
}
