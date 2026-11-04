// Copyright © 2026 Jalapeno Labs

//! Learnings, their history, and their place in the knowledge index.
//!
//! A learning is written for the next agent: a takeaway title, a body that leads with the rule,
//! the key of the category it is filed under (`crate::models::learning_category`), and its
//! evidence (`docs/knowledge.md`, Learnings). A title names one learning, whatever its case, so
//! [`save`] revises the learning that has it and creates one only when none does.
//!
//! Every write takes the [`Author`] making it, locks the learning's row, and records its history
//! in the same transaction, as action items do (`crate::models::action_item_event`): an entry
//! holds the values before and after, so [`undo`] can put back what any one change wrote. The
//! same transaction indexes the learning, so it is searchable the moment the write commits, and
//! a deleted one leaves the index until it is restored. A write that changes nothing records
//! nothing and leaves the index alone.
//!
//! Deleting is soft: a deleted learning keeps its row and history and can be restored.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::database::schema::{learning_events, learnings};
use crate::knowledge::passages;
use crate::models::agent::Agent;
use crate::models::knowledge_document::{self, IndexedDocument, KnowledgeSource};
use crate::models::learning_category;

diesel::define_sql_function! {
    /// Postgres's `lower`, by which `learnings_title_unique` makes a title name one learning.
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

/// Who makes a change, and when.
#[derive(Debug, Clone, Copy)]
pub struct Author<'a> {
    /// The person, or the machine user standing for an agent of Elysium's own.
    pub user_id: Uuid,
    /// The agent acting for them, if one is.
    pub agent: Option<&'a Agent>,
    pub at: DateTime<Utc>,
}

/// A stored learning.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = learnings, check_for_backend(diesel::pg::Pg))]
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
    pub fn content(&self) -> LearningContent {
        LearningContent {
            title: self.title.clone(),
            body: self.body.clone(),
            category: self.category.clone(),
            evidence: self.evidence.clone(),
        }
    }
}

/// What a history entry records, and so how its `data` is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// The learning was first saved. `data` is its content as saved.
    Created,
    /// Its content changed. `data.changes` maps each field to `{ from, to }`, and `data.undid`
    /// names the entry an undo reversed.
    Revised,
    /// `data` is empty, or `{ undid }`.
    Deleted,
    /// `data` is `{ deletedAt }`, the deletion it reversed, and `undid` for an undo.
    Restored,
}

impl HistoryKind {
    /// The name stored in `kind` and sent to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Revised => "revised",
            Self::Deleted => "deleted",
            Self::Restored => "restored",
        }
    }

    /// The kind a stored name stands for; `None` for one a later version wrote.
    fn parse(name: &str) -> Option<Self> {
        [Self::Created, Self::Revised, Self::Deleted, Self::Restored]
            .into_iter()
            .find(|kind| kind.as_str() == name)
    }
}

/// A stored history entry.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = learning_events, check_for_backend(diesel::pg::Pg))]
pub struct HistoryEntry {
    pub id: Uuid,
    pub learning_id: Uuid,
    /// A [`HistoryKind`] name. Kept as text so entries written by later versions still load.
    pub kind: String,
    /// Who made the change.
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub data: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = learning_events)]
struct HistoryRow<'a> {
    id: Uuid,
    learning_id: Uuid,
    kind: &'static str,
    created_by: Uuid,
    agent: Option<&'a str>,
    agent_name: Option<&'a str>,
    data: Value,
    created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = learnings)]
struct LearningRow<'a> {
    id: Uuid,
    title: String,
    body: String,
    category: String,
    evidence: String,
    agent: Option<&'a str>,
    agent_name: Option<&'a str>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(AsChangeset)]
#[diesel(table_name = learnings)]
struct ContentChangeset {
    title: String,
    body: String,
    category: String,
    evidence: String,
}

/// A learning as a write left it, with the history entries the write recorded.
#[derive(Debug)]
pub struct Recorded {
    pub learning: Learning,
    /// Oldest first; empty when the write turned out to change nothing.
    pub history: Vec<HistoryEntry>,
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
}

/// [`DieselError`]'s unique violation, which on `learnings` is only ever a taken title.
fn title_taken(error: DieselError) -> LearningError {
    match error {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
            LearningError::TitleTaken
        }
        other => other.into(),
    }
}

/// Saves `content`: revises the learning with its title, bringing it back if it was deleted, or
/// creates one when no learning has it.
///
/// # Errors
/// Propagates any database error, including a check violation for content the request
/// validation should have refused.
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
            let existing = learnings::table
                .filter(lower(learnings::title).eq(lower(&content.title)))
                .for_update()
                .select(Learning::as_select())
                .first(connection)
                .await
                .optional()?;
            let Some(mut learning) = existing else {
                return create(connection, content, author).await;
            };

            let mut history = Vec::new();
            if learning.deleted_at.is_some() {
                let (restored, entry) =
                    set_deleted(connection, &learning, false, author, None).await?;
                learning = restored;
                history.push(entry);
            }
            if let Some((revised, entry)) =
                revise(connection, &learning, content, author, None).await?
            {
                learning = revised;
                history.push(entry);
            }
            if !history.is_empty() {
                reindex(connection, &learning).await?;
            }
            Ok(Recorded { learning, history })
        })
        .await
}

async fn create(
    connection: &mut AsyncPgConnection,
    content: LearningContent,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    hold_category(connection, &content.category).await?;
    let created = serde_json::to_value(&content).expect("learning content serializes");
    let learning = diesel::insert_into(learnings::table)
        .values(LearningRow {
            id: Uuid::now_v7(),
            title: content.title,
            body: content.body,
            category: content.category,
            evidence: content.evidence,
            agent: author.agent.map(Agent::id),
            agent_name: author.agent.and_then(Agent::name),
            created_by: author.user_id,
            created_at: author.at,
        })
        .returning(Learning::as_returning())
        .get_result(connection)
        .await
        .map_err(title_taken)?;
    let entry = record(
        connection,
        learning.id,
        HistoryKind::Created,
        author,
        created,
    )
    .await?;
    reindex(connection, &learning).await?;
    Ok(Recorded {
        learning,
        history: vec![entry],
    })
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
            let learning = lock(connection, id).await?;
            if learning.deleted_at.is_some() {
                return Err(LearningError::Deleted);
            }
            let current = learning.content();
            let corrected = LearningContent {
                title: changes.title.unwrap_or(current.title),
                body: changes.body.unwrap_or(current.body),
                category: changes.category.unwrap_or(current.category),
                evidence: changes.evidence.unwrap_or(current.evidence),
            };
            let Some((learning, entry)) =
                revise(connection, &learning, corrected, author, None).await?
            else {
                return Ok(Recorded {
                    learning,
                    history: Vec::new(),
                });
            };
            reindex(connection, &learning).await?;
            Ok(Recorded {
                learning,
                history: vec![entry],
            })
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
    set_deleted_by_id(connection, id, true, author).await
}

/// Brings deleted learning `id` back, into the index again. Restoring a live learning changes
/// nothing.
///
/// # Errors
/// `NotFound` for an unknown learning.
pub async fn restore(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    set_deleted_by_id(connection, id, false, author).await
}

async fn set_deleted_by_id(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    deleted: bool,
    author: Author<'_>,
) -> Result<Recorded, LearningError> {
    connection
        .transaction(async move |connection| {
            let learning = lock(connection, id).await?;
            if learning.deleted_at.is_some() == deleted {
                return Ok(Recorded {
                    learning,
                    history: Vec::new(),
                });
            }
            let (learning, entry) =
                set_deleted(connection, &learning, deleted, author, None).await?;
            reindex(connection, &learning).await?;
            Ok(Recorded {
                learning,
                history: vec![entry],
            })
        })
        .await
}

/// Reverses history entry `entry_id` of learning `id`, recording the reversal as a change of its
/// own that names the entry.
///
/// A revision is undone field by field: each field that still holds what it wrote gets back
/// what it replaced, and a field changed again since keeps its newer value, as undoing a
/// changeset does (`docs/action-items.md`, Undoing). Undoing the first save or a restore deletes
/// the learning, and undoing a delete restores it.
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
    connection
        .transaction(async move |connection| {
            let learning = lock(connection, id).await?;
            let entry: HistoryEntry = learning_events::table
                .find(entry_id)
                .filter(learning_events::learning_id.eq(id))
                .select(HistoryEntry::as_select())
                .first(connection)
                .await?;
            let undid = Some(entry.id);
            let is_deleted = learning.deleted_at.is_some();

            let (learning, entry) = match HistoryKind::parse(&entry.kind) {
                Some(HistoryKind::Created | HistoryKind::Restored) if is_deleted => {
                    return Err(LearningError::NothingToUndo(
                        "the learning is already deleted",
                    ));
                }
                Some(HistoryKind::Created | HistoryKind::Restored) => {
                    set_deleted(connection, &learning, true, author, undid).await?
                }
                Some(HistoryKind::Deleted) if !is_deleted => {
                    return Err(LearningError::NothingToUndo("the learning is not deleted"));
                }
                Some(HistoryKind::Deleted) => {
                    set_deleted(connection, &learning, false, author, undid).await?
                }
                Some(HistoryKind::Revised) if is_deleted => return Err(LearningError::Deleted),
                Some(HistoryKind::Revised) => {
                    let reverted = reverted_content(&learning.content(), &entry.data)?;
                    revise(connection, &learning, reverted, author, undid)
                        .await
                        .map_err(|error| match error {
                            LearningError::UnknownCategory { .. } => LearningError::NothingToUndo(
                                "the category this change would put back has been removed",
                            ),
                            other => other,
                        })?
                        .ok_or(LearningError::NothingToUndo(
                            "every field this change wrote has changed again since",
                        ))?
                }
                None => {
                    return Err(LearningError::NothingToUndo(
                        "this kind of change cannot be undone",
                    ));
                }
            };
            reindex(connection, &learning).await?;
            Ok(Recorded {
                learning,
                history: vec![entry],
            })
        })
        .await
}

/// Writes `content` over `learning`'s and records the revision, or answers `None` when it
/// changes nothing. `undid` names the entry this revision reverses, for an undo.
async fn revise(
    connection: &mut AsyncPgConnection,
    learning: &Learning,
    content: LearningContent,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> Result<Option<(Learning, HistoryEntry)>, LearningError> {
    let changes = changes_between(&learning.content(), &content);
    if changes.is_empty() {
        return Ok(None);
    }
    if content.category != learning.category {
        hold_category(connection, &content.category).await?;
    }
    let revised = diesel::update(learnings::table.find(learning.id))
        .set(ContentChangeset {
            title: content.title,
            body: content.body,
            category: content.category,
            evidence: content.evidence,
        })
        .returning(Learning::as_returning())
        .get_result(connection)
        .await
        .map_err(title_taken)?;

    let mut data = json!({ "changes": changes });
    if let Some(undid) = undid {
        data["undid"] = json!(undid);
    }
    let entry = record(connection, learning.id, HistoryKind::Revised, author, data).await?;
    Ok(Some((revised, entry)))
}

/// Moves every learning filed under category `from`, deleted ones included, to category `to`,
/// each move a revision in its history by `author`. Part of the caller's transaction, which
/// must already hold `to` ([`learning_category::hold`]), as removing `from` does.
///
/// # Errors
/// Propagates any database error.
pub async fn refile(
    connection: &mut AsyncPgConnection,
    from: &str,
    to: &str,
    author: Author<'_>,
) -> Result<Vec<Recorded>, LearningError> {
    let filed: Vec<Learning> = learnings::table
        .filter(learnings::category.eq(from))
        .order(learnings::id.asc())
        .for_update()
        .select(Learning::as_select())
        .load(connection)
        .await?;
    let mut moved = Vec::with_capacity(filed.len());
    for learning in filed {
        let content = LearningContent {
            category: to.to_owned(),
            ..learning.content()
        };
        let Some((learning, entry)) = revise(connection, &learning, content, author, None).await?
        else {
            unreachable!("a learning filed under `from` changes when it moves to `to`");
        };
        reindex(connection, &learning).await?;
        moved.push(Recorded {
            learning,
            history: vec![entry],
        });
    }
    Ok(moved)
}

/// Refuses `key` unless a category has it, which then stays until the transaction ends, so a
/// learning is never filed under a category being removed at the same moment.
async fn hold_category(connection: &mut AsyncPgConnection, key: &str) -> Result<(), LearningError> {
    if learning_category::hold(connection, key).await? {
        return Ok(());
    }
    let known = learning_category::list(connection)
        .await?
        .into_iter()
        .map(|category| category.key)
        .collect();
    Err(LearningError::UnknownCategory {
        key: key.to_owned(),
        known,
    })
}

/// Deletes `learning` softly or restores it, and records which.
async fn set_deleted(
    connection: &mut AsyncPgConnection,
    learning: &Learning,
    deleted: bool,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> Result<(Learning, HistoryEntry), LearningError> {
    let (deleted_at, kind, mut data) = if deleted {
        (Some(author.at), HistoryKind::Deleted, json!({}))
    } else {
        let data = json!({ "deletedAt": learning.deleted_at });
        (None, HistoryKind::Restored, data)
    };
    if let Some(undid) = undid {
        data["undid"] = json!(undid);
    }
    let changed = diesel::update(learnings::table.find(learning.id))
        .set(learnings::deleted_at.eq(deleted_at))
        .returning(Learning::as_returning())
        .get_result(connection)
        .await?;
    let entry = record(connection, learning.id, kind, author, data).await?;
    Ok((changed, entry))
}

/// Puts `learning` in the index as it now stands, or takes it out while it is deleted.
async fn reindex(connection: &mut AsyncPgConnection, learning: &Learning) -> QueryResult<()> {
    if learning.deleted_at.is_some() {
        return knowledge_document::remove(connection, KnowledgeSource::Learning(learning.id))
            .await;
    }
    knowledge_document::upsert(connection, &indexed_document(learning))
        .await
        .map(drop)
}

/// The document a learning is indexed as: its title, body, and evidence, in that order, cut
/// into passages. A learning lives in Elysium, so its citation opens it there by id rather than
/// following a link.
fn indexed_document(learning: &Learning) -> IndexedDocument {
    let text = format!(
        "{}\n\n{}\n\n{}",
        learning.title, learning.body, learning.evidence
    );
    IndexedDocument {
        source: KnowledgeSource::Learning(learning.id),
        title: learning.title.clone(),
        url: None,
        // When it last changed, which is when what it now says was said.
        occurred_at: learning.updated_at,
        created_by: learning.created_by,
        passages: passages::split(&text),
    }
}

async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Learning> {
    learnings::table
        .find(id)
        .for_update()
        .select(Learning::as_select())
        .first(connection)
        .await
}

async fn record(
    connection: &mut AsyncPgConnection,
    learning_id: Uuid,
    kind: HistoryKind,
    author: Author<'_>,
    data: Value,
) -> QueryResult<HistoryEntry> {
    diesel::insert_into(learning_events::table)
        .values(HistoryRow {
            id: Uuid::now_v7(),
            learning_id,
            kind: kind.as_str(),
            created_by: author.user_id,
            agent: author.agent.map(Agent::id),
            agent_name: author.agent.and_then(Agent::name),
            data,
            created_at: author.at,
        })
        .returning(HistoryEntry::as_returning())
        .get_result(connection)
        .await
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

/// `current` with each field a revision's `data.changes` wrote, and that still holds what it
/// wrote, put back to what it replaced.
fn reverted_content(
    current: &LearningContent,
    data: &Value,
) -> Result<LearningContent, LearningError> {
    const UNREADABLE: &str = "this change's record cannot be read";
    let Value::Object(mut fields) =
        serde_json::to_value(current).expect("learning content serializes")
    else {
        unreachable!("a struct serializes as an object");
    };
    let changes = data
        .get("changes")
        .and_then(Value::as_object)
        .ok_or(LearningError::NothingToUndo(UNREADABLE))?;
    for (field, change) in changes {
        let (Some(from), Some(to)) = (change.get("from"), change.get("to")) else {
            return Err(LearningError::NothingToUndo(UNREADABLE));
        };
        if fields.get(field) == Some(to) {
            fields.insert(field.clone(), from.clone());
        }
    }
    serde_json::from_value(Value::Object(fields))
        .map_err(|_unreadable_field| LearningError::NothingToUndo(UNREADABLE))
}

/// Learning `id`, live or deleted.
///
/// # Errors
/// `NotFound` for an unknown learning.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Learning> {
    learnings::table
        .find(id)
        .select(Learning::as_select())
        .first(connection)
        .await
}

/// Live learnings, most recently changed first; or deleted ones, most recently deleted first.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection, deleted: bool) -> QueryResult<Vec<Learning>> {
    let query = learnings::table.select(Learning::as_select());
    if deleted {
        return query
            .filter(learnings::deleted_at.is_not_null())
            .order(learnings::deleted_at.desc())
            .load(connection)
            .await;
    }
    query
        .filter(learnings::deleted_at.is_null())
        .order(learnings::updated_at.desc())
        .load(connection)
        .await
}

/// Learning `id`'s history, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn history(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Vec<HistoryEntry>> {
    learning_events::table
        .filter(learning_events::learning_id.eq(id))
        .order(learning_events::id.asc())
        .select(HistoryEntry::as_select())
        .load(connection)
        .await
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
    let mut query = learning_events::table
        .inner_join(learnings::table)
        .select((HistoryEntry::as_select(), Learning::as_select()))
        .order(learning_events::id.desc())
        .limit(limit)
        .into_boxed();
    if let Some(before) = before {
        query = query.filter(learning_events::id.lt(before));
    }
    query.load(connection).await
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
    fn undoing_a_revision_puts_back_only_the_fields_that_still_hold_what_it_wrote() {
        let original = content();
        let revised = LearningContent {
            body: "Revised body.".to_owned(),
            category: "pitfall".to_owned(),
            ..content()
        };
        let data = json!({ "changes": changes_between(&original, &revised) });
        // Since the revision, someone changed the category again.
        let current = LearningContent {
            category: "reference".to_owned(),
            ..revised
        };

        let reverted = reverted_content(&current, &data).expect("reverts");

        assert_eq!(
            reverted.body, original.body,
            "the body it wrote is put back"
        );
        assert_eq!(
            reverted.category, "reference",
            "a field changed since keeps its newer value"
        );
        assert_eq!(
            reverted_content(&original, &data).expect("reverts"),
            original,
            "an undone revision has nothing left to put back"
        );
    }

    #[test]
    fn an_unreadable_revision_is_refused_rather_than_guessed_at() {
        for data in [
            json!({}),
            json!({ "changes": { "body": { "from": "a" } } }),
            json!({ "changes": { "category": { "from": 7, "to": "procedure" } } }),
        ] {
            assert!(
                matches!(
                    reverted_content(&content(), &data),
                    Err(LearningError::NothingToUndo(_))
                ),
                "{data} is refused"
            );
        }
    }
}
