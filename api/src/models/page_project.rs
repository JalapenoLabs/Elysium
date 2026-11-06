// Copyright © 2026 Jalapeno Labs

//! Links between pages and projects, how a project shows its docs in a tree with no spaces.
//!
//! A page may be linked to any number of projects (`docs/knowledge.md`, Pages). A link is no version:
//! it changes nothing a page says, so it is an entry in the page's history alone, and it leaves the
//! knowledge index as it is, which reads links as they stand when a search filters by project. It
//! changes nobody's access: every member reads every page, and no guest reads any, linked or not.
//!
//! Every write takes the [`Author`] making it. Agents may link any page, a proposal-only one too:
//! that rule guards what the company doc says and where it sits, which a link does not touch, and
//! so does a fixed page's. A link already there, or one already gone, records nothing.
//!
//! Each write holds the project before it locks the page, the order deleting a project takes them
//! in ([`remove_all_from_project`]), so the two never wait on each other. A deleted page keeps its
//! links and takes no new ones, so it comes back as it went.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use uuid::Uuid;

use crate::database::schema::{page_projects, page_versions, pages, projects};
use crate::models::agent::Author;
use crate::models::page::{
    self, HistoryEntry, HistoryKind, Page, PageError, PageSummary, Recorded,
};

/// A live page linked to a project, as the project lists it.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct ProjectPage {
    #[diesel(embed)]
    pub page: PageSummary,
    /// Who published its newest version, and when.
    #[diesel(select_expression = page_versions::created_by)]
    pub published_by: Uuid,
    #[diesel(select_expression = page_versions::created_at)]
    pub published_at: DateTime<Utc>,
    /// Who linked it to the project, and when.
    #[diesel(select_expression = page_projects::created_by)]
    pub linked_by: Uuid,
    #[diesel(select_expression = page_projects::created_at)]
    pub linked_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = page_projects)]
struct LinkRow {
    page_id: Uuid,
    project_id: Uuid,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

/// Links page `page_id` to project `project_id`, recording it in the page's history. A link already
/// there records nothing.
///
/// # Errors
/// `NotFound` for an unknown page or project, and [`PageError::Deleted`] for a deleted page.
pub async fn link(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    project_id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let project_name = hold_project(connection, project_id).await?;
            let page = lock_live(connection, page_id).await?;
            let linked = diesel::insert_into(page_projects::table)
                .values(LinkRow {
                    page_id,
                    project_id,
                    created_by: author.user_id,
                    created_at: author.at,
                })
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
            if linked == 0 {
                return Ok(Recorded::unchanged(page));
            }
            let entry = record(
                connection,
                page_id,
                HistoryKind::ProjectLinked,
                (project_id, &project_name),
                author,
            )
            .await?;
            Ok(Recorded::of(page, entry))
        })
        .await
}

/// Removes page `page_id`'s link to project `project_id`, recording it in the page's history. A link
/// that is not there records nothing.
///
/// # Errors
/// `NotFound` for an unknown page or project, and [`PageError::Deleted`] for a deleted page.
pub async fn unlink(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    project_id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let project_name = hold_project(connection, project_id).await?;
            let page = lock_live(connection, page_id).await?;
            let unlinked = diesel::delete(page_projects::table.find((page_id, project_id)))
                .execute(connection)
                .await?;
            if unlinked == 0 {
                return Ok(Recorded::unchanged(page));
            }
            let entry = record(
                connection,
                page_id,
                HistoryKind::ProjectUnlinked,
                (project_id, &project_name),
                author,
            )
            .await?;
            Ok(Recorded::of(page, entry))
        })
        .await
}

/// Takes every page off project `project_id`, for deleting it, recording it in each page's history,
/// deleted pages included. The caller holds the project ([`crate::models::project::lock_for_delete`])
/// in its transaction, so nothing links to it meanwhile. Answers each page's change.
///
/// # Errors
/// Propagates any database error, and `NotFound` for an unknown project.
pub async fn remove_all_from_project(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    author: Author<'_>,
) -> QueryResult<Vec<Recorded>> {
    let project_name: String = projects::table
        .find(project_id)
        .select(projects::name)
        .first(connection)
        .await?;
    let unlinked: Vec<Uuid> =
        diesel::delete(page_projects::table.filter(page_projects::project_id.eq(project_id)))
            .returning(page_projects::page_id)
            .get_results(connection)
            .await?;
    let linked_pages: Vec<Page> = pages::table
        .filter(pages::id.eq_any(&unlinked))
        .order(pages::id.asc())
        .for_update()
        .select(Page::as_select())
        .load(connection)
        .await?;

    let mut recorded = Vec::with_capacity(linked_pages.len());
    for page in linked_pages {
        let entry = record(
            connection,
            page.id,
            HistoryKind::ProjectUnlinked,
            (project_id, &project_name),
            author,
        )
        .await?;
        recorded.push(Recorded::of(page, entry));
    }
    Ok(recorded)
}

/// The projects page `page_id` is linked to, oldest link first.
///
/// # Errors
/// Propagates any database error.
pub async fn projects_of(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
) -> QueryResult<Vec<Uuid>> {
    page_projects::table
        .filter(page_projects::page_id.eq(page_id))
        .order((
            page_projects::created_at.asc(),
            page_projects::project_id.asc(),
        ))
        .select(page_projects::project_id)
        .load(connection)
        .await
}

/// The live pages linked to project `project_id`, newest published first. A deleted page keeps its
/// link and comes back on the list with its page.
///
/// # Errors
/// Propagates any database error.
pub async fn pages_of(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Vec<ProjectPage>> {
    page_projects::table
        .inner_join(pages::table)
        .inner_join(
            page_versions::table.on(page_versions::page_id
                .eq(pages::id)
                .and(page_versions::version.eq(pages::current_version))),
        )
        .filter(page_projects::project_id.eq(project_id))
        .filter(pages::deleted_at.is_null())
        .order((page_versions::created_at.desc(), pages::id.asc()))
        .select(ProjectPage::as_select())
        .load(connection)
        .await
}

/// Live page `page_id` whole, if it is linked to project `project_id`.
///
/// # Errors
/// Propagates any database error.
pub async fn linked_page(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    page_id: Uuid,
) -> QueryResult<Option<Page>> {
    page_projects::table
        .inner_join(pages::table)
        .filter(page_projects::project_id.eq(project_id))
        .filter(page_projects::page_id.eq(page_id))
        .filter(pages::deleted_at.is_null())
        .select(Page::as_select())
        .first(connection)
        .await
        .optional()
}

/// Holds project `id` until the transaction ends, so it cannot be deleted under a link being
/// written, and answers its name for the page's history.
async fn hold_project(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<String> {
    projects::table
        .find(id)
        .for_key_share()
        .select(projects::name)
        .first(connection)
        .await
}

/// Locks page `id` and refuses it while it is deleted: a deleted page changes no further.
async fn lock_live(connection: &mut AsyncPgConnection, id: Uuid) -> Result<Page, PageError> {
    let page = page::lock(connection, id).await?;
    if page.deleted_at.is_some() {
        return Err(PageError::Deleted);
    }
    Ok(page)
}

/// Records a change to a link in page `page_id`'s history, naming the project as it is called now.
async fn record(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    kind: HistoryKind,
    (project_id, project_name): (Uuid, &str),
    author: Author<'_>,
) -> QueryResult<HistoryEntry> {
    let data = json!({ "projectId": project_id, "projectName": project_name });
    page::record(connection, page_id, None, kind, author, data).await
}
