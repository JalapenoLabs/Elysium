// Copyright © 2026 Jalapeno Labs

//! Knowledge base pages: one tree, every published version, and each page's history.
//!
//! A page is Markdown in one tree for the whole workspace (`docs/knowledge.md`, Pages). Publishing
//! is the only write of its content: [`create`] publishes version 1, [`publish`] the next, and
//! [`restore_version`] an earlier version's content as a new one, so `pages` always holds a copy of
//! its newest row in `page_versions`. Everything a version holds is a [`PageContent`].
//!
//! Every write takes the [`Author`] making it and, in one transaction, locks the page's row, records
//! an entry in its history, and puts it in the knowledge index or takes it out, so a page is
//! searchable the moment it publishes. A write that changes nothing records nothing, leaves the
//! index alone, and answers an empty [`Recorded::changed`].
//!
//! Three rules hold the tree together, and every change to its shape takes one lock
//! ([`hold_tree`]) to keep them: a live page's parent is live, a page is never under itself at any
//! depth, and siblings never tie. Content needs no such lock: a publish changes one page's row.
//!
//! Deleting is soft and takes a page's descendants with it, giving them all one `deleted_at`.
//! Restoring brings back the ones that carry its instant, so a child deleted on its own beforehand
//! stays deleted.

use chrono::{DateTime, Utc};
use diesel::dsl::max;
use diesel::prelude::*;
use diesel::result::Error as DieselError;
use diesel::sql_types::{BigInt, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::{page_events, page_versions, pages};
use crate::knowledge::passages;
use crate::models::agent::{Agent, Author};
use crate::models::knowledge_document::{self, IndexedDocument, KnowledgeSource};
use crate::models::user;

/// What the search's source filter calls a page's document.
const INDEX_KIND: &str = "page";

/// The distance between siblings as they are appended and renumbered.
///
/// A page lands between two others at their midpoint, writing its own row alone, so about twenty
/// pages can be dropped into one spot before it runs out of room and [`renumber`] spreads that
/// parent's children again. Small enough that positions stay exact in JavaScript's numbers for any
/// tree people could build, which is what clients sort them in.
const POSITION_GAP: i64 = 1 << 20;

/// The key of the advisory lock every change to the tree's shape takes.
const TREE_LOCK: &str = "pages:tree";

/// Everything a version holds, and so what restoring one puts back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageContent {
    pub title: String,
    pub body: String,
    /// The key of the learning category it is filed under, on a page that is a learning.
    pub category: Option<String>,
    /// What shows a learning is true, on a page that is one.
    pub evidence: Option<String>,
}

/// What a person or an agent writes when they publish. The rest of a page's content stays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageEdit {
    /// On one line, trimmed and its spacing collapsed.
    pub title: String,
    /// Markdown; may be empty.
    pub body: String,
}

/// Where a page goes: under `parent_id`, or at the top of the tree, right after sibling `after`, or
/// first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    pub parent_id: Option<Uuid>,
    pub after: Option<Uuid>,
}

/// A stored page, as its newest version left it.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = pages, check_for_backend(diesel::pg::Pg))]
pub struct Page {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub position: i64,
    pub title: String,
    pub body: String,
    pub category: Option<String>,
    pub evidence: Option<String>,
    pub current_version: i32,
    /// Agents may only propose changes to it (`docs/knowledge.md`, Pages).
    pub proposal_only: bool,
    pub deleted_at: Option<DateTime<Utc>>,
    /// Who created it. Who changed it since is in its versions and history.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Page {
    fn content(&self) -> PageContent {
        PageContent {
            title: self.title.clone(),
            body: self.body.clone(),
            category: self.category.clone(),
            evidence: self.evidence.clone(),
        }
    }
}

/// A page as the tree lists it, without what it says.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = pages, check_for_backend(diesel::pg::Pg))]
pub struct PageSummary {
    pub id: Uuid,
    pub parent_id: Option<Uuid>,
    pub position: i64,
    pub title: String,
    pub current_version: i32,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A version without what it says: who published it, and when.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = page_versions, check_for_backend(diesel::pg::Pg))]
pub struct VersionSummary {
    pub page_id: Uuid,
    pub version: i32,
    pub title: String,
    /// The version a restore copied.
    pub restored_from: Option<i32>,
    /// Who published it.
    pub created_by: Uuid,
    /// The agent acting for them, in its recorded form.
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A version whole.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = page_versions, check_for_backend(diesel::pg::Pg))]
pub struct PageVersion {
    #[diesel(embed)]
    pub summary: VersionSummary,
    pub body: String,
    pub category: Option<String>,
    pub evidence: Option<String>,
}

impl PageVersion {
    fn into_content(self) -> PageContent {
        PageContent {
            title: self.summary.title,
            body: self.body,
            category: self.category,
            evidence: self.evidence,
        }
    }
}

/// What a history entry records, and so how its `data` is shaped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// The page was created. `data` is `{ parentId, version }`.
    Created,
    /// A version was published. `data` is `{ version }`, with `restoredFrom` for a restore.
    Published,
    /// It moved, or changed place among its siblings. `data` is `{ from, to }`, each
    /// `{ parentId, after }`: the two values a move takes, so putting a page back is a move.
    Moved,
    /// `data` is empty, or `{ deletedWith }`, the ancestor whose deletion took it.
    Deleted,
    /// `data` is `{ deletedAt }`, the deletion it reversed, with `restoredWith`, the ancestor
    /// whose restore brought it back.
    Restored,
}

impl HistoryKind {
    /// The name stored in `kind` and sent to clients.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Published => "published",
            Self::Moved => "moved",
            Self::Deleted => "deleted",
            Self::Restored => "restored",
        }
    }
}

/// A stored history entry.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = page_events, check_for_backend(diesel::pg::Pg))]
pub struct HistoryEntry {
    pub id: Uuid,
    pub page_id: Uuid,
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
#[diesel(table_name = pages)]
struct PageRow<'a> {
    id: Uuid,
    parent_id: Option<Uuid>,
    position: i64,
    title: &'a str,
    body: &'a str,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = page_versions)]
struct VersionRow<'a> {
    page_id: Uuid,
    version: i32,
    title: &'a str,
    body: &'a str,
    category: Option<&'a str>,
    evidence: Option<&'a str>,
    restored_from: Option<i32>,
    created_by: Uuid,
    agent: Option<&'a str>,
    agent_name: Option<&'a str>,
    created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = page_events)]
struct HistoryRow<'a> {
    id: Uuid,
    page_id: Uuid,
    kind: &'static str,
    created_by: Uuid,
    agent: Option<&'a str>,
    agent_name: Option<&'a str>,
    data: Value,
    created_at: DateTime<Utc>,
}

/// One row of a query answering page ids.
#[derive(QueryableByName)]
struct PageId {
    #[diesel(sql_type = SqlUuid)]
    id: Uuid,
}

/// A page as a write left it.
#[derive(Debug)]
pub struct Recorded {
    pub page: Page,
    /// Every page clients must load again: empty when the write turned out to change nothing, and
    /// more than the page itself when its descendants or its siblings changed with it.
    pub changed: Vec<Uuid>,
}

impl Recorded {
    fn of(page: Page) -> Self {
        let changed = vec![page.id];
        Self { page, changed }
    }

    const fn unchanged(page: Page) -> Self {
        Self {
            page,
            changed: Vec::new(),
        }
    }
}

/// Why a write to a page was refused.
#[derive(Debug, thiserror::Error)]
pub enum PageError {
    /// Includes [`DieselError::NotFound`] for an unknown page or version.
    #[error(transparent)]
    Database(#[from] DieselError),
    /// The page was published since the version the edit was made from.
    #[error("the page changed since this was opened")]
    Changed,
    #[error("this page is deleted; restore it to change it")]
    Deleted,
    /// An agent tried to write a page agents only propose to.
    #[error("people edit this page directly, and agents propose changes to it")]
    ProposalOnly,
    /// The parent named does not exist or is deleted.
    #[error("the page it would go under does not exist or is deleted")]
    ParentGone,
    /// The sibling a move names to land after is not a live child of the parent it names.
    #[error("the page it would follow is no longer under that parent")]
    PlacementGone,
    /// The parent a move names is the page itself or one of its descendants.
    #[error("a page cannot move under itself")]
    UnderItself,
}

/// Creates a page from `edit`, last among `parent_id`'s children, as its version 1.
///
/// # Errors
/// [`PageError::ParentGone`] for a parent that does not exist or is deleted.
pub async fn create(
    connection: &mut AsyncPgConnection,
    parent_id: Option<Uuid>,
    edit: PageEdit,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            hold_tree(connection).await?;
            if let Some(parent_id) = parent_id {
                require_live_parent(connection, parent_id).await?;
            }
            // Deleted siblings count, so a restored page never meets this one in its place.
            let last: Option<i64> = pages::table
                .filter(pages::parent_id.is_not_distinct_from(parent_id))
                .select(max(pages::position))
                .first(connection)
                .await?;
            let position = between(last, None).expect("the end always has room");

            let page: Page = diesel::insert_into(pages::table)
                .values(PageRow {
                    id: Uuid::now_v7(),
                    parent_id,
                    position,
                    title: &edit.title,
                    body: &edit.body,
                    created_by: author.user_id,
                    created_at: author.at,
                })
                .returning(Page::as_returning())
                .get_result(connection)
                .await?;
            insert_version(connection, &page, &page.content(), None, author).await?;
            let data = json!({ "parentId": parent_id, "version": page.current_version });
            record(connection, page.id, HistoryKind::Created, author, data).await?;
            reindex(connection, &page).await?;
            Ok(Recorded::of(page))
        })
        .await
}

/// Publishes `edit` over page `id` as its next version.
///
/// `base_version` is the version the edit was made from. Content the page already holds records
/// nothing, whatever it was made from, since nothing would be overwritten: an unchanged page
/// published again, or two people publishing the same text, makes no version.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::Deleted`] for a deleted one,
/// [`PageError::ProposalOnly`] for an agent writing a page agents only propose to, and
/// [`PageError::Changed`] when a version was published since `base_version`.
pub async fn publish(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    edit: PageEdit,
    base_version: i32,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = lock(connection, id).await?;
            require_writable(connection, &page, author).await?;
            if edit.title == page.title && edit.body == page.body {
                return Ok(Recorded::unchanged(page));
            }
            if base_version != page.current_version {
                return Err(PageError::Changed);
            }
            let content = PageContent {
                title: edit.title,
                body: edit.body,
                ..page.content()
            };
            write_version(connection, &page, &content, None, author).await
        })
        .await
}

/// Publishes version `version` of page `id` as its next version, naming the one it copied, so
/// nothing is lost. A version whose content the page already holds records nothing.
///
/// It names no base version, unlike [`publish`]: what it replaces stays a version of its own, so
/// restoring over someone's change costs them nothing.
///
/// # Errors
/// `NotFound` for an unknown page or version, [`PageError::Deleted`] for a deleted page, and
/// [`PageError::ProposalOnly`] for an agent writing a page agents only propose to.
pub async fn restore_version(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    version: i32,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = lock(connection, id).await?;
            require_writable(connection, &page, author).await?;
            let content = find_version(connection, id, version).await?.into_content();
            if content == page.content() {
                return Ok(Recorded::unchanged(page));
            }
            write_version(connection, &page, &content, Some(version), author).await
        })
        .await
}

/// Moves page `id` to `placement`, with everything under it, writing its own row alone unless its
/// new neighbours have no room between them. A page already there records nothing.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::Deleted`] for a deleted one,
/// [`PageError::ProposalOnly`] for an agent writing a page agents only propose to,
/// [`PageError::ParentGone`] for a parent that does not exist or is deleted,
/// [`PageError::UnderItself`] for a parent that is the page or a descendant of it, and
/// [`PageError::PlacementGone`] when `after` is not a live child of the parent.
pub async fn move_page(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    placement: Placement,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            hold_tree(connection).await?;
            let page = lock(connection, id).await?;
            require_writable(connection, &page, author).await?;
            if let Some(parent_id) = placement.parent_id {
                require_live_parent(connection, parent_id).await?;
                if subtree(connection, id).await?.contains(&parent_id) {
                    return Err(PageError::UnderItself);
                }
            }

            let follows = sibling_before(connection, &page).await?;
            let stays_under_its_parent = placement.parent_id == page.parent_id;
            if stays_under_its_parent && (placement.after == follows || placement.after == Some(id))
            {
                return Ok(Recorded::unchanged(page));
            }

            let (position, renumbered) = position_at(connection, placement, id).await?;
            let moved = diesel::update(pages::table.find(id))
                .set((
                    pages::parent_id.eq(placement.parent_id),
                    pages::position.eq(position),
                ))
                .returning(Page::as_returning())
                .get_result(connection)
                .await?;
            let data = json!({
                "from": { "parentId": page.parent_id, "after": follows },
                "to": { "parentId": placement.parent_id, "after": placement.after },
            });
            record(connection, id, HistoryKind::Moved, author, data).await?;

            let mut recorded = Recorded::of(moved);
            recorded
                .changed
                .extend(renumbered.into_iter().filter(|sibling| *sibling != id));
            Ok(recorded)
        })
        .await
}

/// Deletes page `id` softly with every live page under it, taking them all out of the index.
/// Deleting a deleted page changes nothing.
///
/// # Errors
/// `NotFound` for an unknown page, and [`PageError::ProposalOnly`] for an agent when any of them
/// is a page agents only propose to, in which case none is deleted.
pub async fn delete(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            hold_tree(connection).await?;
            let page = lock(connection, id).await?;
            if page.deleted_at.is_some() {
                return Ok(Recorded::unchanged(page));
            }

            let under = subtree(connection, id).await?;
            let going: Vec<Page> = pages::table
                .filter(pages::id.eq_any(&under))
                .filter(pages::deleted_at.is_null())
                .order(pages::id.asc())
                .for_update()
                .select(Page::as_select())
                .load(connection)
                .await?;
            for page in &going {
                refuse_agents(connection, page, author).await?;
            }
            let changed: Vec<Uuid> = going.iter().map(|page| page.id).collect();
            diesel::update(pages::table.filter(pages::id.eq_any(&changed)))
                .set(pages::deleted_at.eq(author.at))
                .execute(connection)
                .await?;

            for page_id in &changed {
                let data = if *page_id == id {
                    json!({})
                } else {
                    json!({ "deletedWith": id })
                };
                record(connection, *page_id, HistoryKind::Deleted, author, data).await?;
                knowledge_document::remove(connection, &index_source(*page_id)).await?;
            }
            let page = find(connection, id).await?;
            Ok(Recorded { page, changed })
        })
        .await
}

/// Brings deleted page `id` back to where it was, with the descendants its deletion took, into the
/// index again. A descendant deleted on its own beforehand stays deleted. Restoring a live page
/// changes nothing.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::ParentGone`] while its parent is deleted, since a
/// live page's parent is always live, and [`PageError::ProposalOnly`] for an agent when any of them
/// is a page agents only propose to, in which case none is restored.
pub async fn restore(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            hold_tree(connection).await?;
            let page = lock(connection, id).await?;
            let Some(deleted_at) = page.deleted_at else {
                return Ok(Recorded::unchanged(page));
            };
            if let Some(parent_id) = page.parent_id {
                require_live_parent(connection, parent_id).await?;
            }

            let under = subtree(connection, id).await?;
            let returning: Vec<Page> = pages::table
                .filter(pages::id.eq_any(&under))
                .filter(pages::deleted_at.eq(deleted_at))
                .order(pages::id.asc())
                .for_update()
                .select(Page::as_select())
                .load(connection)
                .await?;
            for page in &returning {
                refuse_agents(connection, page, author).await?;
            }
            let changed: Vec<Uuid> = returning.iter().map(|page| page.id).collect();
            let restored: Vec<Page> =
                diesel::update(pages::table.filter(pages::id.eq_any(&changed)))
                    .set(pages::deleted_at.eq(None::<DateTime<Utc>>))
                    .returning(Page::as_returning())
                    .get_results(connection)
                    .await?;

            for page in &restored {
                let data = if page.id == id {
                    json!({ "deletedAt": deleted_at })
                } else {
                    json!({ "deletedAt": deleted_at, "restoredWith": id })
                };
                record(connection, page.id, HistoryKind::Restored, author, data).await?;
                reindex(connection, page).await?;
            }
            let page = find(connection, id).await?;
            Ok(Recorded { page, changed })
        })
        .await
}

/// Page `id`, live or deleted.
///
/// # Errors
/// `NotFound` for an unknown page.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Page> {
    pages::table
        .find(id)
        .select(Page::as_select())
        .first(connection)
        .await
}

/// Every live page, in order among its siblings, for a client to build the tree from; or every
/// deleted one, most recently deleted first.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    deleted: bool,
) -> QueryResult<Vec<PageSummary>> {
    let query = pages::table.select(PageSummary::as_select());
    if deleted {
        return query
            .filter(pages::deleted_at.is_not_null())
            .order((pages::deleted_at.desc(), pages::id.asc()))
            .load(connection)
            .await;
    }
    query
        .filter(pages::deleted_at.is_null())
        .order(pages::position.asc())
        .load(connection)
        .await
}

/// Page `id`'s versions, newest first, without what they say.
///
/// # Errors
/// Propagates any database error.
pub async fn versions(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Vec<VersionSummary>> {
    page_versions::table
        .filter(page_versions::page_id.eq(id))
        .order(page_versions::version.desc())
        .select(VersionSummary::as_select())
        .load(connection)
        .await
}

/// Who published version `version` of page `id`, and when.
///
/// # Errors
/// `NotFound` for an unknown page or version.
pub async fn version_summary(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    version: i32,
) -> QueryResult<VersionSummary> {
    page_versions::table
        .find((id, version))
        .select(VersionSummary::as_select())
        .first(connection)
        .await
}

/// Version `version` of page `id`, whole.
///
/// # Errors
/// `NotFound` for an unknown page or version.
pub async fn find_version(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    version: i32,
) -> QueryResult<PageVersion> {
    page_versions::table
        .find((id, version))
        .select(PageVersion::as_select())
        .first(connection)
        .await
}

/// Page `id`'s history, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn history(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<Vec<HistoryEntry>> {
    page_events::table
        .filter(page_events::page_id.eq(id))
        .order(page_events::id.asc())
        .select(HistoryEntry::as_select())
        .load(connection)
        .await
}

/// Takes the lock every change to the tree's shape holds until its transaction ends.
///
/// Row locks cannot keep the tree's rules, which span pages no one row names: two moves at once
/// could each put a page under the other's descendant, and each would look fine alone. People
/// change the tree's shape rarely enough that doing it one at a time costs nothing.
async fn hold_tree(connection: &mut AsyncPgConnection) -> QueryResult<()> {
    diesel::sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind::<Text, _>(TREE_LOCK)
        .execute(connection)
        .await
        .map(drop)
}

async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Page> {
    pages::table
        .find(id)
        .for_update()
        .select(Page::as_select())
        .first(connection)
        .await
}

/// Refuses `parent_id` unless it is a live page. The caller holds the tree, so it stays one.
async fn require_live_parent(
    connection: &mut AsyncPgConnection,
    parent_id: Uuid,
) -> Result<(), PageError> {
    let parent: Option<Uuid> = pages::table
        .find(parent_id)
        .filter(pages::deleted_at.is_null())
        .select(pages::id)
        .first(connection)
        .await
        .optional()?;
    parent.map(drop).ok_or(PageError::ParentGone)
}

/// Refuses a change to `page`'s content or place while it is deleted or agents may not write it.
async fn require_writable(
    connection: &mut AsyncPgConnection,
    page: &Page,
    author: Author<'_>,
) -> Result<(), PageError> {
    if page.deleted_at.is_some() {
        return Err(PageError::Deleted);
    }
    refuse_agents(connection, page, author).await
}

/// Refuses an agent writing a page agents only propose to.
///
/// Anything but a person acting alone counts. Elysia writes as her machine user with no agent named,
/// so the user is read too, and a caller that forgets to name its agent is refused all the same.
async fn refuse_agents(
    connection: &mut AsyncPgConnection,
    page: &Page,
    author: Author<'_>,
) -> Result<(), PageError> {
    if !page.proposal_only {
        return Ok(());
    }
    if author.agent.is_some() || user::find(connection, author.user_id).await?.is_machine() {
        return Err(PageError::ProposalOnly);
    }
    Ok(())
}

/// Page `id` and every page under it, deleted ones included, at any depth.
async fn subtree(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Vec<Uuid>> {
    let rows: Vec<PageId> = diesel::sql_query(
        "WITH RECURSIVE subtree AS ( \
             SELECT id FROM pages WHERE id = $1 \
             UNION ALL \
             SELECT pages.id FROM pages JOIN subtree ON pages.parent_id = subtree.id \
         ) \
         SELECT id FROM subtree",
    )
    .bind::<SqlUuid, _>(id)
    .load(connection)
    .await?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}

/// The live sibling just before `page`, which a [`Placement`] names as `after`.
async fn sibling_before(
    connection: &mut AsyncPgConnection,
    page: &Page,
) -> QueryResult<Option<Uuid>> {
    pages::table
        .filter(pages::parent_id.is_not_distinct_from(page.parent_id))
        .filter(pages::deleted_at.is_null())
        .filter(pages::position.lt(page.position))
        .order(pages::position.desc())
        .select(pages::id)
        .first(connection)
        .await
        .optional()
}

/// The position `placement` means for page `moving`, and the siblings renumbered to make room.
async fn position_at(
    connection: &mut AsyncPgConnection,
    placement: Placement,
    moving: Uuid,
) -> Result<(i64, Vec<Uuid>), PageError> {
    let (lower, upper) = neighbours(connection, placement, moving).await?;
    if let Some(position) = between(lower, upper) {
        return Ok((position, Vec::new()));
    }
    event!(
        name: "pages.position.renumbered",
        Level::DEBUG,
        page.id = %moving,
        "no room between two pages; renumbering their siblings",
    );
    let renumbered = renumber(connection, placement.parent_id).await?;
    let (lower, upper) = neighbours(connection, placement, moving).await?;
    let position = between(lower, upper).expect("renumbered siblings are a gap apart");
    Ok((position, renumbered))
}

/// The positions either side of `placement`, leaving out page `moving`. Deleted siblings count, so
/// a page never takes the place one comes back to.
async fn neighbours(
    connection: &mut AsyncPgConnection,
    placement: Placement,
    moving: Uuid,
) -> Result<(Option<i64>, Option<i64>), PageError> {
    let lower = match placement.after {
        None => None,
        Some(after) => {
            let position: Option<i64> = pages::table
                .find(after)
                .filter(pages::parent_id.is_not_distinct_from(placement.parent_id))
                .filter(pages::deleted_at.is_null())
                .select(pages::position)
                .first(connection)
                .await
                .optional()?;
            Some(position.ok_or(PageError::PlacementGone)?)
        }
    };

    let mut after_lower = pages::table
        .filter(pages::parent_id.is_not_distinct_from(placement.parent_id))
        .filter(pages::id.ne(moving))
        .order(pages::position.asc())
        .select(pages::position)
        .into_boxed();
    if let Some(lower) = lower {
        after_lower = after_lower.filter(pages::position.gt(lower));
    }
    let upper = after_lower.first(connection).await.optional()?;
    Ok((lower, upper))
}

/// A position after `lower` and before `upper`, either of which may be missing at the ends of the
/// siblings, or `None` when the two touch and have no room between them.
fn between(lower: Option<i64>, upper: Option<i64>) -> Option<i64> {
    match (lower, upper) {
        (None, None) => Some(POSITION_GAP),
        (Some(lower), None) => Some(lower + POSITION_GAP),
        (None, Some(upper)) => Some(upper - POSITION_GAP),
        (Some(lower), Some(upper)) => {
            let middle = lower + (upper - lower) / 2;
            (middle != lower).then_some(middle)
        }
    }
}

/// Spreads `parent_id`'s children a gap apart again, keeping their order, deleted ones included,
/// and answers which they are. One statement, so `pages_sibling_positions` is checked when it ends.
async fn renumber(
    connection: &mut AsyncPgConnection,
    parent_id: Option<Uuid>,
) -> QueryResult<Vec<Uuid>> {
    let rows: Vec<PageId> = diesel::sql_query(
        "UPDATE pages SET position = ranked.rank * $2 \
         FROM ( \
             SELECT id, row_number() OVER (ORDER BY position) AS rank \
             FROM pages WHERE parent_id IS NOT DISTINCT FROM $1 \
         ) ranked \
         WHERE pages.id = ranked.id \
         RETURNING pages.id",
    )
    .bind::<Nullable<SqlUuid>, _>(parent_id)
    .bind::<BigInt, _>(POSITION_GAP)
    .load(connection)
    .await?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}

/// Writes `content` as `page`'s next version: its version row, the copy on the page, the history
/// entry, and the index, together. `restored_from` names the version a restore copied.
async fn write_version(
    connection: &mut AsyncPgConnection,
    page: &Page,
    content: &PageContent,
    restored_from: Option<i32>,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    let version = page.current_version + 1;
    let published: Page = diesel::update(pages::table.find(page.id))
        .set((
            pages::title.eq(&content.title),
            pages::body.eq(&content.body),
            pages::category.eq(&content.category),
            pages::evidence.eq(&content.evidence),
            pages::current_version.eq(version),
        ))
        .returning(Page::as_returning())
        .get_result(connection)
        .await?;
    insert_version(connection, &published, content, restored_from, author).await?;

    let mut data = json!({ "version": version });
    if let Some(restored_from) = restored_from {
        data["restoredFrom"] = json!(restored_from);
    }
    record(connection, page.id, HistoryKind::Published, author, data).await?;
    reindex(connection, &published).await?;
    Ok(Recorded::of(published))
}

/// Stores `content` as `page`'s current version.
async fn insert_version(
    connection: &mut AsyncPgConnection,
    page: &Page,
    content: &PageContent,
    restored_from: Option<i32>,
    author: Author<'_>,
) -> QueryResult<()> {
    diesel::insert_into(page_versions::table)
        .values(VersionRow {
            page_id: page.id,
            version: page.current_version,
            title: &content.title,
            body: &content.body,
            category: content.category.as_deref(),
            evidence: content.evidence.as_deref(),
            restored_from,
            created_by: author.user_id,
            agent: author.agent.map(Agent::id),
            agent_name: author.agent.and_then(Agent::name),
            created_at: author.at,
        })
        .execute(connection)
        .await
        .map(drop)
}

async fn record(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    kind: HistoryKind,
    author: Author<'_>,
    data: Value,
) -> QueryResult<()> {
    diesel::insert_into(page_events::table)
        .values(HistoryRow {
            id: Uuid::now_v7(),
            page_id,
            kind: kind.as_str(),
            created_by: author.user_id,
            agent: author.agent.map(Agent::id),
            agent_name: author.agent.and_then(Agent::name),
            data,
            created_at: author.at,
        })
        .execute(connection)
        .await
        .map(drop)
}

fn index_source(id: Uuid) -> KnowledgeSource {
    KnowledgeSource::Page {
        id,
        kind: INDEX_KIND.to_owned(),
    }
}

/// Puts `page`, which is live, in the index as its newest version reads.
///
/// Dated when that version was published, and attributed to who published it, since those are its
/// words now. A page lives in Elysium, so its citation opens it there by id rather than following a
/// link.
async fn reindex(connection: &mut AsyncPgConnection, page: &Page) -> QueryResult<()> {
    let published = version_summary(connection, page.id, page.current_version).await?;
    let document = IndexedDocument {
        source: index_source(page.id),
        title: page.title.clone(),
        url: None,
        occurred_at: published.created_at,
        created_by: published.created_by,
        passages: passages::split(&indexed_text(page)),
    };
    knowledge_document::upsert(connection, &document)
        .await
        .map(drop)
}

/// What a page is indexed from: its title and body, then its evidence on a page that has any, in
/// the order a learning's document holds them.
fn indexed_text(page: &Page) -> String {
    let mut text = format!("{}\n\n{}", page.title, page.body);
    if let Some(evidence) = &page.evidence {
        text.push_str("\n\n");
        text.push_str(evidence);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page() -> Page {
        Page {
            id: Uuid::now_v7(),
            parent_id: None,
            position: POSITION_GAP,
            title: "Runbook".to_owned(),
            body: "Restart it.".to_owned(),
            category: None,
            evidence: None,
            current_version: 1,
            proposal_only: false,
            deleted_at: None,
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn a_page_lands_a_gap_past_either_end_and_midway_between_two() {
        assert_eq!(between(None, None), Some(POSITION_GAP));
        assert_eq!(between(Some(8), None), Some(8 + POSITION_GAP));
        assert_eq!(between(None, Some(8)), Some(8 - POSITION_GAP));
        assert_eq!(between(Some(8), Some(12)), Some(10));
        assert_eq!(between(Some(-3), Some(-1)), Some(-2));
    }

    #[test]
    fn two_pages_that_touch_have_no_room_between_them() {
        assert_eq!(between(Some(8), Some(9)), None);
    }

    #[test]
    fn about_twenty_pages_fit_into_one_spot_before_it_runs_out_of_room() {
        let lower = POSITION_GAP;
        let mut upper = 2 * POSITION_GAP;
        let mut placed = 0;
        while let Some(position) = between(Some(lower), Some(upper)) {
            upper = position;
            placed += 1;
        }
        assert_eq!(placed, 20);
    }

    #[test]
    fn a_page_is_indexed_from_its_title_and_body() {
        assert_eq!(indexed_text(&page()), "Runbook\n\nRestart it.");
    }

    #[test]
    fn a_page_with_evidence_is_indexed_as_a_learning_is() {
        let learning = Page {
            evidence: Some("It came back.".to_owned()),
            ..page()
        };
        assert_eq!(
            indexed_text(&learning),
            "Runbook\n\nRestart it.\n\nIt came back."
        );
    }
}
