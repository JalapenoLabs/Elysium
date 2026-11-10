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
//! stays deleted. [`delete_keeping_children`] instead moves a page's children into its place, and
//! restoring it moves them back.
//!
//! A learning is a page with a category and evidence (`crate::models::learning`), and the company
//! doc is seven pages under one (`crate::models::company_doc`). Both are written through
//! [`publish_content`] and [`undo_version`], so they keep their own rules over the versions, history,
//! and index every page has. The pages they rely on are fixed: nobody moves, deletes, or renames one.
//!
//! A project's brief is a page too (`crate::models::project_brief`), fixed while its project exists,
//! and publishing it as anyone but its own Project Manager or the system user wakes that Project
//! Manager in the same transaction ([`Recorded::woke`]).
//!
//! Labels are no part of a version: `crate::models::page_label` writes them, as history alone.

use chrono::{DateTime, Utc};
use diesel::dsl::max;
use diesel::prelude::*;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel::sql_types::{BigInt, Nullable, Text, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::{Uuid, uuid};

use crate::database::schema::{page_events, page_versions, pages, projects};
use crate::knowledge::passages;
use crate::models::agent::{Agent, Author};
use crate::models::company_doc::{CONTENT_MAX_CHARACTERS, Section};
use crate::models::knowledge_document::{self, IndexedDocument, KnowledgeSource};
use crate::models::{learning_category, project_brief, project_manager, user};
use crate::search::{self, SearchTerms};

/// The page every learning is created under, seeded by its migration at this id.
pub const LEARNINGS_PAGE_ID: Uuid = uuid!("00000000-0000-0000-0001-000000000001");

/// The page the company doc's sections sit under, seeded by its migration at this id. Each section's
/// own page is [`Section::page_id`].
pub const COMPANY_PAGE_ID: Uuid = uuid!("00000000-0000-0000-0001-000000000002");

/// The page every project's brief sits under, seeded by its migration at this id.
pub const PROJECT_BRIEFS_PAGE_ID: Uuid = uuid!("00000000-0000-0000-0001-000000000003");

/// The page every meeting is listed under, seeded by its migration at this id. Meetings are no
/// pages: the web app lists the ones its reader can see under it, as their home
/// (`docs/knowledge.md`, The tree).
pub const MEETINGS_PAGE_ID: Uuid = uuid!("00000000-0000-0000-0001-000000000004");

/// What the search's source filter calls each kind of page's document. A learning's and a section's
/// keep the names they had before they were pages, which clients filter by.
const INDEX_KIND: &str = "page";
pub const LEARNING_INDEX_KIND: &str = "learning";
const COMPANY_DOC_INDEX_KIND: &str = "company_doc";

/// The longest a learning's body may be, in characters, as `pages_learning_body_fits` allows.
const LEARNING_BODY_MAX_CHARACTERS: usize = 20_000;

/// The unique index that makes a title name one learning.
const LEARNING_TITLE_INDEX: &str = "pages_learning_title_unique";

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

/// What a person or an agent writes when they publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageEdit {
    /// On one line, trimmed and its spacing collapsed.
    pub title: String,
    /// Markdown; may be empty.
    pub body: String,
    /// What a learning holds beyond a title and body. `None` leaves a learning's as they are.
    pub learning: Option<LearningFields>,
}

/// What makes a page a learning: always both, as `pages_learning_fields_together` holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearningFields {
    /// The key of the category it is filed under.
    pub category: String,
    /// What shows it is true.
    pub evidence: String,
}

/// What a version's history entry says beyond its number.
#[derive(Debug, Clone, Copy, Default)]
pub struct Provenance {
    /// The version a restore copied.
    pub restored_from: Option<i32>,
    /// The entry this one reverses, for an undo.
    pub undid: Option<Uuid>,
    /// The company doc proposal it applied.
    pub proposal_id: Option<Uuid>,
    /// The id of the one change this is part of, when a change reaches several pages. A change that
    /// reaches one page takes an id of its own.
    pub change: Option<Uuid>,
}

impl PageEdit {
    /// The content a new page holds.
    fn into_content(self) -> PageContent {
        let (category, evidence) = self
            .learning
            .map(|fields| (fields.category, fields.evidence))
            .unzip();
        PageContent {
            title: self.title,
            body: self.body,
            category,
            evidence,
        }
    }

    /// The content `page` holds with this written over it.
    fn over(self, page: &Page) -> PageContent {
        let (category, evidence) = self.learning.map_or_else(
            || (page.category.clone(), page.evidence.clone()),
            |fields| (Some(fields.category), Some(fields.evidence)),
        );
        PageContent {
            title: self.title,
            body: self.body,
            category,
            evidence,
        }
    }
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
    /// The project whose brief it is, while that project exists.
    pub brief_project_id: Option<Uuid>,
}

impl Page {
    pub fn content(&self) -> PageContent {
        PageContent {
            title: self.title.clone(),
            body: self.body.clone(),
            category: self.category.clone(),
            evidence: self.evidence.clone(),
        }
    }

    /// Whether this page is a learning, wherever in the tree it sits.
    pub const fn is_learning(&self) -> bool {
        self.category.is_some()
    }

    /// The company doc section this page holds, if it is one.
    pub fn section(&self) -> Option<Section> {
        Section::of_page(self.id)
    }

    /// Whether Elysium relies on this page, so nobody moves, deletes, or renames it, people
    /// included: one it seeds ([`is_seeded`]), or a project's brief, which its Project Manager and
    /// every batch's goal check find by its project while that project exists.
    pub fn is_fixed(&self) -> bool {
        is_seeded(self.id) || self.brief_project_id.is_some()
    }
}

/// Whether page `id` is one Elysium seeds at an id fixed in code: the Learnings, Company, Project
/// briefs, and Meetings pages and the doc's sections.
pub fn is_seeded(id: Uuid) -> bool {
    id == LEARNINGS_PAGE_ID
        || id == COMPANY_PAGE_ID
        || id == PROJECT_BRIEFS_PAGE_ID
        || id == MEETINGS_PAGE_ID
        || Section::of_page(id).is_some()
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
    pub updated_at: DateTime<Utc>,
    /// The project whose brief it is, while that project exists.
    pub brief_project_id: Option<Uuid>,
}

impl PageSummary {
    /// Whether Elysium relies on this page ([`Page::is_fixed`]).
    pub fn is_fixed(&self) -> bool {
        is_seeded(self.id) || self.brief_project_id.is_some()
    }
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
    pub fn into_content(self) -> PageContent {
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
    /// A version was published. `data` is `{ version }`, with `restoredFrom` for a restore, `undid`
    /// for an undo, and `proposalId` for an approved company doc proposal.
    Published,
    /// It moved, or changed place among its siblings. `data` is `{ from, to }`, each
    /// `{ parentId, after }`: the two values a move takes, so putting a page back is a move.
    Moved,
    /// `data` is empty, or `{ deletedWith }`, the ancestor whose deletion took it, with `undid` for
    /// an undo, or `{ keptChildren }`, each child moved into its place and the position it left.
    Deleted,
    /// `data` is `{ deletedAt }`, the deletion it reversed, with `restoredWith`, the ancestor
    /// whose restore brought it back, and `undid` for an undo.
    Restored,
    /// A label was put on it. `data` is `{ label }`.
    Labeled,
    /// A label was taken off it. `data` is `{ label }`.
    Unlabeled,
    /// A file was attached, or put back after being removed. `data` is `{ attachmentId, name }`.
    Attached,
    /// A file was taken off it. `data` is `{ attachmentId, name }`.
    Detached,
    /// It was linked to a project. `data` is `{ projectId, projectName }`, the name as it was then,
    /// since the entry outlives a project deleted later.
    ProjectLinked,
    /// Its link to a project went, by hand or with the project. `data` is `{ projectId, projectName }`.
    ProjectUnlinked,
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
            Self::Labeled => "labeled",
            Self::Unlabeled => "unlabeled",
            Self::Attached => "attached",
            Self::Detached => "detached",
            Self::ProjectLinked => "project_linked",
            Self::ProjectUnlinked => "project_unlinked",
        }
    }

    /// The kind a stored name stands for; `None` for one a later version wrote.
    pub fn parse(name: &str) -> Option<Self> {
        [
            Self::Created,
            Self::Published,
            Self::Moved,
            Self::Deleted,
            Self::Restored,
            Self::Labeled,
            Self::Unlabeled,
            Self::Attached,
            Self::Detached,
            Self::ProjectLinked,
            Self::ProjectUnlinked,
        ]
        .into_iter()
        .find(|kind| kind.as_str() == name)
    }
}

/// A stored history entry.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(
    table_name = page_events,
    primary_key(page_id, id),
    check_for_backend(diesel::pg::Pg)
)]
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

impl HistoryEntry {
    /// The version an entry of [`HistoryKind::Published`] wrote.
    pub fn published_version(&self) -> Option<i32> {
        let version = self.data.get("version")?.as_i64()?;
        i32::try_from(version).ok()
    }
}

#[derive(Insertable)]
#[diesel(table_name = pages)]
struct PageRow<'a> {
    id: Uuid,
    parent_id: Option<Uuid>,
    position: i64,
    title: &'a str,
    body: &'a str,
    category: Option<&'a str>,
    evidence: Option<&'a str>,
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
    /// Those of them that are learnings, whose own event clients hear too.
    pub learnings: Vec<Uuid>,
    /// What the write recorded of the page itself, oldest first; empty when it changed nothing.
    pub history: Vec<HistoryEntry>,
    /// The project whose Project Manager the write woke: someone else published its brief. The
    /// caller signals the dispatcher once the write commits.
    pub woke: Option<Uuid>,
}

impl Recorded {
    pub(super) fn of(page: Page, entry: HistoryEntry) -> Self {
        Self {
            changed: vec![page.id],
            learnings: page.is_learning().then_some(page.id).into_iter().collect(),
            history: vec![entry],
            woke: None,
            page,
        }
    }

    pub(super) const fn unchanged(page: Page) -> Self {
        Self {
            page,
            changed: Vec::new(),
            learnings: Vec::new(),
            history: Vec::new(),
            woke: None,
        }
    }

    /// `page`, after a write that reached every one of `touched`, recording `entry` on it.
    fn of_many(page: Page, touched: &[Page], entry: Option<HistoryEntry>) -> Self {
        Self {
            page,
            changed: touched.iter().map(|page| page.id).collect(),
            learnings: touched
                .iter()
                .filter(|page| page.is_learning())
                .map(|page| page.id)
                .collect(),
            history: entry.into_iter().collect(),
            woke: None,
        }
    }

    /// Counts `moved`, which this write moved too, and `renumbered`, whose positions it changed.
    fn absorb(&mut self, moved: &[Page], renumbered: Vec<Uuid>) {
        self.changed.extend(moved.iter().map(|page| page.id));
        self.changed.extend(renumbered);
        self.changed.sort_unstable();
        self.changed.dedup();
        self.learnings.extend(
            moved
                .iter()
                .filter(|page| page.is_learning())
                .map(|page| page.id),
        );
    }
}

/// A child [`delete_keeping_children`] moved into its page's place, and the position under that page
/// it left, which the page's `deleted` entry keeps so that restoring the page puts it back exactly.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct KeptChild {
    id: Uuid,
    position: i64,
}

/// What moving a page's children into its place did.
#[derive(Debug, Default)]
struct LiftedChildren {
    /// The children, as they now are.
    moved: Vec<Page>,
    kept: Vec<KeptChild>,
    /// The siblings renumbered to make room.
    renumbered: Vec<Uuid>,
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
    #[error("people edit this page directly; agents propose changes with company_doc_propose")]
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
    /// The write would move, delete, or rename a page Elysium relies on ([`Page::is_fixed`]).
    #[error("this page holds its place: it cannot be moved, deleted, or renamed")]
    Fixed,
    /// The write would take a project's brief off that project.
    #[error("a project's brief stays linked to its project")]
    BriefStaysLinked,
    /// Another learning, deleted ones included, has the title.
    #[error("another learning already has this title")]
    TitleTaken,
    /// No category has the key a write named. `known` is every key there is, in the order they are
    /// offered, so the agent or person who named it can choose again.
    #[error("`{key}` is not a learning category; use one of: {}", known.join(", "))]
    UnknownCategory { key: String, known: Vec<String> },
    /// A publish named a category and evidence for a page that is no learning.
    #[error("only a learning has a category and evidence")]
    NotALearning,
    /// What a learning or a company doc section says is outside what it may hold.
    #[error("this page's body is {min} to {max} characters")]
    BodyOutOfBounds { min: usize, max: usize },
    /// The page already carries as many labels as one page may.
    #[error("a page carries at most {max} labels")]
    TooManyLabels { max: i64 },
    /// The page already holds as many attachments as one page may.
    #[error("a page holds at most {max} attachments")]
    TooManyAttachments { max: i64 },
    /// No admin has chosen a storage location for attachments.
    #[error("no storage location is chosen for attachments")]
    NoAttachmentLocation,
}

/// [`DieselError`]'s unique violation of [`LEARNING_TITLE_INDEX`], and any other error as it is.
fn title_taken(error: DieselError) -> PageError {
    match &error {
        DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, information)
            if information.constraint_name() == Some(LEARNING_TITLE_INDEX) =>
        {
            PageError::TitleTaken
        }
        _ => error.into(),
    }
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

            let content = edit.into_content();
            require_fits(None, &content)?;
            if let Some(category) = &content.category {
                hold_category(connection, category).await?;
            }
            let page: Page = diesel::insert_into(pages::table)
                .values(PageRow {
                    id: Uuid::now_v7(),
                    parent_id,
                    position,
                    title: &content.title,
                    body: &content.body,
                    category: content.category.as_deref(),
                    evidence: content.evidence.as_deref(),
                    created_by: author.user_id,
                    created_at: author.at,
                })
                .returning(Page::as_returning())
                .get_result(connection)
                .await
                .map_err(title_taken)?;
            insert_version(connection, &page, &content, None, author).await?;
            let data = json!({ "parentId": parent_id, "version": page.current_version });
            let entry = record(
                connection,
                page.id,
                None,
                HistoryKind::Created,
                author,
                data,
            )
            .await?;
            reindex(connection, &page).await?;
            Ok(Recorded::of(page, entry))
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
/// [`PageError::ProposalOnly`] for an agent writing a page agents only propose to,
/// [`PageError::Changed`] when a version was published since `base_version`, [`PageError::Fixed`]
/// for a new title on a fixed page, [`PageError::NotALearning`] for a category and evidence on a page
/// that is no learning, and the refusals of [`publish_content`].
pub async fn publish(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    edit: PageEdit,
    base_version: i32,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = lock_to_publish(connection, id).await?;
            require_writable(connection, &page, author).await?;
            if edit.learning.is_some() && !page.is_learning() {
                return Err(PageError::NotALearning);
            }
            let content = edit.over(&page);
            if content == page.content() {
                return Ok(Recorded::unchanged(page));
            }
            if base_version != page.current_version {
                return Err(PageError::Changed);
            }
            if page.is_fixed() && content.title != page.title {
                return Err(PageError::Fixed);
            }
            publish_content(connection, &page, content, Provenance::default(), author).await
        })
        .await
}

/// Publishes `content` over `page` as its next version, recording `provenance` in its history.
///
/// This is what every publish comes down to, for the writes with rules of their own over a page:
/// saving a learning and changing the company doc. `page` must be locked ([`lock`]) in the caller's
/// transaction, and whether the author may write it is the caller's to decide. A deleted page takes
/// the version and stays out of the index, which is how removing a category moves a deleted learning.
/// A project's brief is locked with its project held first ([`lock_to_publish`]), since publishing
/// one writes its Project Manager's wake.
///
/// # Errors
/// [`PageError::TitleTaken`] for a learning given another's title, [`PageError::UnknownCategory`]
/// for a category that does not exist, and [`PageError::BodyOutOfBounds`] for a body a learning or
/// a section may not hold.
pub async fn publish_content(
    connection: &mut AsyncPgConnection,
    page: &Page,
    content: PageContent,
    provenance: Provenance,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    require_fits(Section::of_page(page.id), &content)?;
    if let Some(category) = content
        .category
        .as_ref()
        .filter(|_| content.category != page.category)
    {
        hold_category(connection, category).await?;
    }

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
        .await
        .map_err(title_taken)?;
    insert_version(
        connection,
        &published,
        &content,
        provenance.restored_from,
        author,
    )
    .await?;

    let mut data = json!({ "version": version });
    if let Some(restored_from) = provenance.restored_from {
        data["restoredFrom"] = json!(restored_from);
    }
    if let Some(undid) = provenance.undid {
        data["undid"] = json!(undid);
    }
    if let Some(proposal_id) = provenance.proposal_id {
        data["proposalId"] = json!(proposal_id);
    }
    let entry = record(
        connection,
        page.id,
        provenance.change,
        HistoryKind::Published,
        author,
        data,
    )
    .await?;
    reindex(connection, &published).await?;

    let mut recorded = Recorded::of(published, entry);
    if let Some(project_id) = page.brief_project_id
        && project_brief::wakes_its_manager(author, project_id)
    {
        project_manager::wake_for_brief(connection, project_id, version, author.user_id).await?;
        recorded.woke = Some(project_id);
    }
    Ok(recorded)
}

/// Publishes version `version` of page `id` as its next version, naming the one it copied, so
/// nothing is lost. A version whose content the page already holds records nothing.
///
/// It names no base version, unlike [`publish`]: what it replaces stays a version of its own, so
/// restoring over someone's change costs them nothing.
///
/// A version filed under a category removed since comes back filed where the learning is now: the
/// category is gone, and refusing would leave what the version says out of reach.
///
/// # Errors
/// `NotFound` for an unknown page or version, [`PageError::Deleted`] for a deleted page,
/// [`PageError::ProposalOnly`] for an agent writing a page agents only propose to, and
/// [`PageError::TitleTaken`] for a learning's version whose title another learning has now.
pub async fn restore_version(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    version: i32,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = lock_to_publish(connection, id).await?;
            require_writable(connection, &page, author).await?;
            let mut content = find_version(connection, id, version).await?.into_content();
            if let Some(category) = &content.category
                && !learning_category::hold(connection, category).await?
            {
                content.category.clone_from(&page.category);
            }
            if content == page.content() {
                return Ok(Recorded::unchanged(page));
            }
            let provenance = Provenance {
                restored_from: Some(version),
                ..Provenance::default()
            };
            publish_content(connection, &page, content, provenance, author).await
        })
        .await
}

/// Reverses what version `version` of `page` changed: each field that still holds what that version
/// wrote gets back what it replaced, and one changed again since keeps its newer value, as undoing a
/// changeset does (`docs/action-items.md`, Undoing). The reversal is the next version, naming what it
/// undid in `provenance`. Answers `None`, recording nothing, when no field still holds what it wrote.
///
/// `page` must be locked ([`lock`]) in the caller's transaction.
///
/// # Errors
/// `NotFound` for an unknown version or one with none before it, and the refusals of
/// [`publish_content`].
pub async fn undo_version(
    connection: &mut AsyncPgConnection,
    page: &Page,
    version: i32,
    provenance: Provenance,
    author: Author<'_>,
) -> Result<Option<Recorded>, PageError> {
    let wrote = find_version(connection, page.id, version)
        .await?
        .into_content();
    let replaced = find_version(connection, page.id, version - 1)
        .await?
        .into_content();
    let reverted = reverted_content(page.content(), &wrote, replaced);
    if reverted == page.content() {
        return Ok(None);
    }
    publish_content(connection, page, reverted, provenance, author)
        .await
        .map(Some)
}

/// Moves page `id` to `placement`, with everything under it, writing its own row alone unless its
/// new neighbours have no room between them. A page already there records nothing.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::Deleted`] for a deleted one,
/// [`PageError::ProposalOnly`] for an agent writing a page agents only propose to,
/// [`PageError::Fixed`] for a fixed page, [`PageError::ParentGone`] for a parent that does not exist
/// or is deleted, [`PageError::UnderItself`] for a parent that is the page or a descendant of it,
/// and [`PageError::PlacementGone`] when `after` is not a live child of the parent.
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
            if page.is_fixed() {
                return Err(PageError::Fixed);
            }
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
            let moved = place(connection, id, placement.parent_id, position).await?;
            let data = moved_data(
                (page.parent_id, follows),
                (placement.parent_id, placement.after),
            );
            let entry = record(connection, id, None, HistoryKind::Moved, author, data).await?;

            let mut recorded = Recorded::of(moved, entry);
            recorded
                .changed
                .extend(renumbered.into_iter().filter(|sibling| *sibling != id));
            Ok(recorded)
        })
        .await
}

/// Deletes page `id` softly with every live page under it, taking them all out of the index.
/// Deleting a deleted page changes nothing. `undid` names the entry this reverses, for an undo.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::ProposalOnly`] for an agent when any of them is a
/// page agents only propose to, and [`PageError::Fixed`] when any of them is a fixed page, in which
/// cases none is deleted.
pub async fn delete(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            hold_tree(connection).await?;
            let page = lock(connection, id).await?;
            if page.deleted_at.is_some() {
                return Ok(Recorded::unchanged(page));
            }

            let data = undid.map_or_else(|| json!({}), |undid| json!({ "undid": undid }));
            take_down(connection, id, Uuid::now_v7(), data, author).await
        })
        .await
}

/// Deletes page `id` softly and alone: its live children take its place among its siblings, in
/// their order, each with everything under it, so nothing but the page leaves the tree. Each move is
/// in that child's history, under the deletion's one change, and restoring the page moves them
/// back. Deleting a deleted page changes nothing.
///
/// # Errors
/// As [`delete`], for the page and each of its children, since moving one writes it too.
pub async fn delete_keeping_children(
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

            let change = Uuid::now_v7();
            let lifted = lift_children(connection, &page, change, author).await?;
            let data = json!({ "keptChildren": lifted.kept });
            let mut recorded = take_down(connection, id, change, data, author).await?;
            recorded.absorb(&lifted.moved, lifted.renumbered);
            Ok(recorded)
        })
        .await
}

/// Deletes page `id` with every live page under it, recording `data` on the page itself as `change`.
/// The caller holds the tree and has found the page live.
async fn take_down(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    change: Uuid,
    data: Value,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
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
    if going.iter().any(Page::is_fixed) {
        return Err(PageError::Fixed);
    }
    let changed: Vec<Uuid> = going.iter().map(|page| page.id).collect();
    diesel::update(pages::table.filter(pages::id.eq_any(&changed)))
        .set(pages::deleted_at.eq(author.at))
        .execute(connection)
        .await?;

    let mut entry_of_page = None;
    for going_page in &going {
        let (entry_id, entry_data) = if going_page.id == id {
            (Some(change), data.clone())
        } else {
            (None, json!({ "deletedWith": id }))
        };
        let entry = record(
            connection,
            going_page.id,
            entry_id,
            HistoryKind::Deleted,
            author,
            entry_data,
        )
        .await?;
        if going_page.id == id {
            entry_of_page = Some(entry);
        }
        knowledge_document::remove(connection, &index_source(going_page)).await?;
    }
    let page = find(connection, id).await?;
    Ok(Recorded::of_many(page, &going, entry_of_page))
}

/// Moves `page`'s live children, in their order, to right after it among its siblings. The caller
/// holds the tree and is about to delete `page`, so they end up where it was.
async fn lift_children(
    connection: &mut AsyncPgConnection,
    page: &Page,
    change: Uuid,
    author: Author<'_>,
) -> Result<LiftedChildren, PageError> {
    let children: Vec<Page> = pages::table
        .filter(pages::parent_id.eq(page.id))
        .filter(pages::deleted_at.is_null())
        .order(pages::position.asc())
        .for_update()
        .select(Page::as_select())
        .load(connection)
        .await?;

    let mut lifted = LiftedChildren::default();
    let mut previous = None;
    // Each lands after the one before it, the first after `page`, which is still live here. Its
    // history names who it follows once `page` is gone.
    let mut lands_after = page.id;
    let mut reads_after = sibling_before(connection, page).await?;
    for child in children {
        refuse_agents(connection, &child, author).await?;
        if child.is_fixed() {
            return Err(PageError::Fixed);
        }

        let placement = Placement {
            parent_id: page.parent_id,
            after: Some(lands_after),
        };
        let (position, renumbered) = position_at(connection, placement, child.id).await?;
        lifted.renumbered.extend(renumbered);
        let moved = place(connection, child.id, page.parent_id, position).await?;
        let data = moved_data((Some(page.id), previous), (page.parent_id, reads_after));
        record(
            connection,
            child.id,
            Some(change),
            HistoryKind::Moved,
            author,
            data,
        )
        .await?;

        lifted.kept.push(KeptChild {
            id: child.id,
            position: child.position,
        });
        lifted.moved.push(moved);
        previous = Some(child.id);
        lands_after = child.id;
        reads_after = Some(child.id);
    }
    Ok(lifted)
}

/// The children the deletion at `deleted_at` left in page `id`'s place, as it recorded them.
async fn kept_children(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    deleted_at: DateTime<Utc>,
) -> Result<Vec<KeptChild>, PageError> {
    let deletion: Option<Value> = page_events::table
        .filter(page_events::page_id.eq(id))
        .filter(page_events::kind.eq(HistoryKind::Deleted.as_str()))
        .filter(page_events::created_at.eq(deleted_at))
        .select(page_events::data)
        .first(connection)
        .await
        .optional()?;
    let Some(kept) = deletion.and_then(|mut data| data.get_mut("keptChildren").map(Value::take))
    else {
        return Ok(Vec::new());
    };
    serde_json::from_value(kept)
        .map_err(|error| DieselError::DeserializationError(Box::new(error)).into())
}

/// Moves `kept`, the children `page`'s deletion moved into its place, back under it at the positions
/// they left, recording each as part of `change`. The caller holds the tree and has restored the
/// page. Nothing can have taken those positions, since a deleted page's children change no further.
/// A child moved or deleted since stays where someone put it.
async fn return_children(
    connection: &mut AsyncPgConnection,
    page: &Page,
    kept: Vec<KeptChild>,
    change: Uuid,
    author: Author<'_>,
) -> Result<Vec<Page>, PageError> {
    let mut returned = Vec::new();
    for kept_child in kept {
        let child = lock(connection, kept_child.id).await?;
        if child.deleted_at.is_some() || child.parent_id != page.parent_id {
            continue;
        }
        refuse_agents(connection, &child, author).await?;

        let follows = sibling_before(connection, &child).await?;
        let moved = place(connection, child.id, Some(page.id), kept_child.position).await?;
        let lands_after = sibling_before(connection, &moved).await?;
        let data = moved_data((child.parent_id, follows), (Some(page.id), lands_after));
        record(
            connection,
            child.id,
            Some(change),
            HistoryKind::Moved,
            author,
            data,
        )
        .await?;
        returned.push(moved);
    }
    Ok(returned)
}

/// Brings deleted page `id` back to where it was, with the descendants its deletion took, into the
/// index again, and with the children [`delete_keeping_children`] moved out of it. A descendant
/// deleted on its own beforehand stays deleted. Restoring a live page changes nothing. `undid` names
/// the entry this reverses, for an undo.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::ParentGone`] while its parent is deleted, since a
/// live page's parent is always live, and [`PageError::ProposalOnly`] for an agent when any of them
/// is a page agents only propose to, in which case none is restored.
pub async fn restore(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    author: Author<'_>,
    undid: Option<Uuid>,
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

            let change = Uuid::now_v7();
            let mut entry_of_page = None;
            for restored_page in &restored {
                let mut data = if restored_page.id == id {
                    json!({ "deletedAt": deleted_at })
                } else {
                    json!({ "deletedAt": deleted_at, "restoredWith": id })
                };
                if let Some(undid) = undid.filter(|_| restored_page.id == id) {
                    data["undid"] = json!(undid);
                }
                let entry = record(
                    connection,
                    restored_page.id,
                    (restored_page.id == id).then_some(change),
                    HistoryKind::Restored,
                    author,
                    data,
                )
                .await?;
                if restored_page.id == id {
                    entry_of_page = Some(entry);
                }
                reindex(connection, restored_page).await?;
            }
            let page = find(connection, id).await?;
            let kept = kept_children(connection, id, deleted_at).await?;
            let returned = return_children(connection, &page, kept, change, author).await?;
            let mut recorded = Recorded::of_many(page, &restored, entry_of_page);
            recorded.absorb(&returned, Vec::new());
            Ok(recorded)
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

/// The `limit` live pages whose titles match `terms` most closely, as id and title, learnings and
/// the company doc's sections among them, since they are pages. See `crate::search` for how
/// titles match and rank.
///
/// # Errors
/// Propagates any database error.
pub async fn search(
    connection: &mut AsyncPgConnection,
    terms: &SearchTerms,
    limit: i64,
) -> QueryResult<Vec<(Uuid, String)>> {
    let mut query = pages::table
        .filter(pages::deleted_at.is_null())
        .into_boxed();
    for pattern in terms.word_patterns() {
        query = query.filter(search::matches(pages::title, pattern));
    }

    query
        .order((
            search::matches(pages::title, terms.title_start_pattern()).desc(),
            search::similarity(pages::title, terms.text().to_owned()).desc(),
            pages::title.asc(),
        ))
        .limit(limit)
        .select((pages::id, pages::title))
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

/// History entry `entry_id` of page `id`.
///
/// # Errors
/// `NotFound` for an unknown entry, or one of another page.
pub async fn history_entry(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    entry_id: Uuid,
) -> QueryResult<HistoryEntry> {
    page_events::table
        .find((id, entry_id))
        .select(HistoryEntry::as_select())
        .first(connection)
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

/// Locks page `id` until the transaction ends, and answers it.
///
/// # Errors
/// `NotFound` for an unknown page.
pub async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Page> {
    pages::table
        .find(id)
        .for_update()
        .select(Page::as_select())
        .first(connection)
        .await
}

/// Locks page `id` for a publish, holding its project first when it is a project's brief.
///
/// Publishing a brief writes its Project Manager's wake, which references the project and so holds
/// its row, while renaming a project holds the project's row and then locks its brief. Holding the
/// project before the page, the order every write that reaches both takes (`page_project`), keeps
/// the two from waiting on each other. A project deleted in between holds nothing, and its brief
/// is then an ordinary page.
async fn lock_to_publish(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Page> {
    let brief_of: Option<Uuid> = pages::table
        .find(id)
        .select(pages::brief_project_id)
        .first(connection)
        .await?;
    if let Some(project_id) = brief_of {
        projects::table
            .find(project_id)
            .for_key_share()
            .select(projects::id)
            .first::<Uuid>(connection)
            .await
            .optional()?;
    }
    lock(connection, id).await
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
pub(super) async fn require_writable(
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

/// Refuses `key` unless a category has it, which then stays until the transaction ends, so a
/// learning is never filed under a category being removed at the same moment.
async fn hold_category(connection: &mut AsyncPgConnection, key: &str) -> Result<(), PageError> {
    if learning_category::hold(connection, key).await? {
        return Ok(());
    }
    let known = learning_category::list(connection)
        .await?
        .into_iter()
        .map(|category| category.key)
        .collect();
    Err(PageError::UnknownCategory {
        key: key.to_owned(),
        known,
    })
}

/// Refuses `content` when a learning or company doc section cannot hold it. `section` is the section
/// it is written to, if any. They keep the bounds they always had; a page's own are the routes'.
fn require_fits(section: Option<Section>, content: &PageContent) -> Result<(), PageError> {
    let (min, max) = match (section, &content.category) {
        (Some(_), _) => (0, CONTENT_MAX_CHARACTERS),
        (None, Some(_)) => (1, LEARNING_BODY_MAX_CHARACTERS),
        (None, None) => return Ok(()),
    };
    if (min..=max).contains(&content.body.chars().count()) {
        return Ok(());
    }
    Err(PageError::BodyOutOfBounds { min, max })
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

/// Puts page `id` under `parent_id` at `position`, and answers it as it now is.
async fn place(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    parent_id: Option<Uuid>,
    position: i64,
) -> QueryResult<Page> {
    diesel::update(pages::table.find(id))
        .set((pages::parent_id.eq(parent_id), pages::position.eq(position)))
        .returning(Page::as_returning())
        .get_result(connection)
        .await
}

/// What a `moved` entry holds: each end as the parent and sibling a move would name to go there.
fn moved_data(from: (Option<Uuid>, Option<Uuid>), to: (Option<Uuid>, Option<Uuid>)) -> Value {
    json!({
        "from": { "parentId": from.0, "after": from.1 },
        "to": { "parentId": to.0, "after": to.1 },
    })
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

/// Records a history entry on page `page_id`, under `change` when it is part of a change that
/// reaches several pages.
pub(super) async fn record(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    change: Option<Uuid>,
    kind: HistoryKind,
    author: Author<'_>,
    data: Value,
) -> QueryResult<HistoryEntry> {
    diesel::insert_into(page_events::table)
        .values(HistoryRow {
            id: change.unwrap_or_else(Uuid::now_v7),
            page_id,
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

/// `current` with each field that holds what a version `wrote` put back to what it `replaced`.
fn reverted_content(
    current: PageContent,
    wrote: &PageContent,
    replaced: PageContent,
) -> PageContent {
    fn revert<Field: PartialEq>(current: Field, wrote: &Field, replaced: Field) -> Field {
        if current == *wrote { replaced } else { current }
    }
    PageContent {
        title: revert(current.title, &wrote.title, replaced.title),
        body: revert(current.body, &wrote.body, replaced.body),
        category: revert(current.category, &wrote.category, replaced.category),
        evidence: revert(current.evidence, &wrote.evidence, replaced.evidence),
    }
}

/// What the index calls `page`'s source. The kind is what the search's source filter names it by.
fn index_source(page: &Page) -> KnowledgeSource {
    let kind = if page.section().is_some() {
        COMPANY_DOC_INDEX_KIND
    } else if page.is_learning() {
        LEARNING_INDEX_KIND
    } else {
        INDEX_KIND
    };
    KnowledgeSource::Page {
        id: page.id,
        kind: kind.to_owned(),
    }
}

/// Puts `page` in the index as its newest version reads, or takes it out while it is deleted or says
/// nothing.
///
/// Dated when that version was published, and attributed to who published it, since those are its
/// words now. A page lives in Elysium, so its citation opens it there by id rather than following a
/// link.
pub(crate) async fn reindex(connection: &mut AsyncPgConnection, page: &Page) -> QueryResult<()> {
    let source = index_source(page);
    let passages = passages::split(&indexed_text(page));
    if page.deleted_at.is_some() || passages.is_empty() {
        return knowledge_document::remove(connection, &source).await;
    }
    let published = version_summary(connection, page.id, page.current_version).await?;
    let document = IndexedDocument {
        source,
        title: indexed_title(page),
        url: None,
        occurred_at: published.created_at,
        created_by: published.created_by,
        passages,
    };
    knowledge_document::upsert(connection, &document)
        .await
        .map(drop)
}

/// What a citation calls `page`. A section's page is titled as the section is, which alone would not
/// say whose glossary a search found.
fn indexed_title(page: &Page) -> String {
    match page.section() {
        Some(section) => format!("Company doc: {}", section.heading()),
        None => page.title.clone(),
    }
}

/// What `page` is indexed from: its title and body, then its evidence on a page that has any, in the
/// order a learning's document holds them.
///
/// A page Elysium seeds is its body alone. Its title is a heading the doc's own structure already
/// gives, and the text its passages were embedded from is what keeps their embeddings when a section
/// is indexed again; an empty one says nothing, and has no document. A project's brief is indexed as
/// any page is, title first, since its title says whose brief a search found, once it says more than
/// the skeleton every new brief holds, which would answer every search for a goal with every brief.
fn indexed_text(page: &Page) -> String {
    if is_seeded(page.id) {
        return page.body.clone();
    }
    if page.brief_project_id.is_some() && page.body == project_brief::TEMPLATE {
        return String::new();
    }
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

    fn content() -> PageContent {
        PageContent {
            title: "Runbook".to_owned(),
            body: "Restart it.".to_owned(),
            category: Some("procedure".to_owned()),
            evidence: Some("It came back.".to_owned()),
        }
    }

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
            brief_project_id: None,
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

    #[test]
    fn a_section_is_indexed_from_its_body_alone_under_the_docs_name_for_it() {
        let glossary = Page {
            id: Section::Glossary.page_id(),
            title: "Glossary".to_owned(),
            body: "- ACME: a client".to_owned(),
            ..page()
        };
        assert_eq!(indexed_text(&glossary), "- ACME: a client");
        assert_eq!(indexed_title(&glossary), "Company doc: Glossary");
        assert_eq!(index_source(&glossary).label(), "company_doc");
    }

    #[test]
    fn a_learnings_document_keeps_the_name_the_filter_knows_it_by() {
        let learning = Page {
            category: Some("procedure".to_owned()),
            evidence: Some("It came back.".to_owned()),
            ..page()
        };
        assert_eq!(index_source(&learning).label(), "learning");
        assert_eq!(index_source(&page()).label(), "page");
    }

    #[test]
    fn an_empty_section_says_nothing_to_index() {
        let empty = Page {
            id: Section::People.page_id(),
            body: String::new(),
            ..page()
        };
        assert!(passages::split(&indexed_text(&empty)).is_empty());
    }

    #[test]
    fn the_pages_elysium_relies_on_are_fixed_and_no_others() {
        assert!(is_seeded(LEARNINGS_PAGE_ID));
        assert!(is_seeded(COMPANY_PAGE_ID));
        assert!(is_seeded(PROJECT_BRIEFS_PAGE_ID));
        assert!(is_seeded(MEETINGS_PAGE_ID));
        for section in Section::ALL {
            assert!(is_seeded(section.page_id()), "{section:?}");
        }
        assert!(!is_seeded(Uuid::now_v7()));

        let ordinary = page();
        assert!(!ordinary.is_fixed());
        let brief = Page {
            brief_project_id: Some(Uuid::now_v7()),
            ..page()
        };
        assert!(brief.is_fixed(), "a project's brief holds its place");
        assert_eq!(
            indexed_text(&brief),
            "Runbook\n\nRestart it.",
            "a brief is indexed as any page is, by its title too"
        );
        let skeleton = Page {
            body: project_brief::TEMPLATE.to_owned(),
            ..brief
        };
        assert!(
            passages::split(&indexed_text(&skeleton)).is_empty(),
            "a brief that says only what every new brief says has nothing to find"
        );
    }

    #[test]
    fn a_learning_and_a_section_keep_their_bounds_and_a_page_has_only_the_routes() {
        let body_of = |length: usize| PageContent {
            body: "x".repeat(length),
            ..content()
        };
        require_fits(None, &body_of(LEARNING_BODY_MAX_CHARACTERS)).expect("fits");
        assert!(matches!(
            require_fits(None, &body_of(LEARNING_BODY_MAX_CHARACTERS + 1)),
            Err(PageError::BodyOutOfBounds { .. })
        ));
        assert!(matches!(
            require_fits(None, &body_of(0)),
            Err(PageError::BodyOutOfBounds { .. })
        ));

        let section = Some(Section::Glossary);
        let plain = |length: usize| PageContent {
            body: "x".repeat(length),
            category: None,
            evidence: None,
            ..content()
        };
        require_fits(section, &plain(0)).expect("fits");
        assert!(matches!(
            require_fits(section, &plain(CONTENT_MAX_CHARACTERS + 1)),
            Err(PageError::BodyOutOfBounds { .. })
        ));
        require_fits(None, &plain(CONTENT_MAX_CHARACTERS + 1)).expect("fits");
    }

    #[test]
    fn undoing_puts_back_only_the_fields_that_still_hold_what_the_version_wrote() {
        let replaced = content();
        let wrote = PageContent {
            body: "Revised body.".to_owned(),
            category: Some("pitfall".to_owned()),
            ..content()
        };
        // Since the version, someone changed the category again.
        let current = PageContent {
            category: Some("reference".to_owned()),
            ..wrote.clone()
        };

        let reverted = reverted_content(current, &wrote, replaced.clone());

        assert_eq!(
            reverted.body, replaced.body,
            "the body it wrote is put back"
        );
        assert_eq!(
            reverted.category.as_deref(),
            Some("reference"),
            "a field changed since keeps its newer value"
        );
        assert_eq!(
            reverted_content(replaced.clone(), &wrote, replaced.clone()),
            replaced,
            "an undone version has nothing left to put back"
        );
    }
}
