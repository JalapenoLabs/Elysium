// Copyright © 2026 Jalapeno Labs

//! Files attached to knowledge base pages, one row per file, its bytes with a storage provider.
//!
//! An attachment is no part of a version: adding or removing one makes none, as a label does not,
//! and records an entry in the page's history under the lock on its row every write of a page
//! takes. A body refers to a file by its address, so a version keeps naming a file whatever happens
//! to the attachment since.
//!
//! Who may attach follows from what an attachment is, and differs from a label. A file shows on its
//! page and is what its text links to, so it is what a page presents: a proposal-only page refuses
//! agents here, as it does their publishes, where a label it would take from anyone.
//!
//! Nothing deletes a file yet. Removing an attachment sets `deleted_at` and leaves the file where it
//! is, so it can be put back, and a deleted page keeps its attachments, served as it reads on, for
//! as long as it stays deleted, which today is forever. Deleting a page for good is where files
//! would finally go: it would read `storage_location_id` and `storage_path` from these rows, removed
//! ones included, delete each file, and then the rows, which no foreign key cascades, so nothing can
//! forget them (`docs/knowledge.md`, Pages, Attachments).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use uuid::Uuid;

use crate::database::schema::page_attachments;
use crate::models::agent::{Agent, Author};
use crate::models::page::{self, HistoryKind, PageError, Recorded};
use crate::models::user;

/// The most attachments one live page holds. Nothing about attachments needs a limit to work; it
/// bounds what a page lists and what an agent's loop can pile on, and no page reads well past it.
pub const MAX_ATTACHMENTS_PER_PAGE: i64 = 200;

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = page_attachments, check_for_backend(diesel::pg::Pg))]
pub struct PageAttachment {
    pub id: Uuid,
    pub page_id: Uuid,
    pub storage_location_id: Uuid,
    pub storage_path: String,
    pub file_name: String,
    pub content_type: String,
    pub size_bytes: i64,
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// A file already written to its storage location, to record against a page.
#[derive(Debug, Clone, Copy)]
pub struct StoredFile<'a> {
    pub id: Uuid,
    pub page_id: Uuid,
    pub storage_location_id: Uuid,
    pub storage_path: &'a str,
    pub file_name: &'a str,
    pub content_type: &'a str,
    pub size_bytes: i64,
}

#[derive(Insertable)]
#[diesel(table_name = page_attachments)]
struct AttachmentRow<'a> {
    id: Uuid,
    page_id: Uuid,
    storage_location_id: Uuid,
    storage_path: &'a str,
    file_name: &'a str,
    content_type: &'a str,
    size_bytes: i64,
    created_by: Uuid,
    agent: Option<&'a str>,
    agent_name: Option<&'a str>,
    created_at: DateTime<Utc>,
}

/// The attachments page `page_id` holds, the oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn of_page(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
) -> QueryResult<Vec<PageAttachment>> {
    page_attachments::table
        .filter(page_attachments::page_id.eq(page_id))
        .filter(page_attachments::deleted_at.is_null())
        .order((page_attachments::created_at, page_attachments::id))
        .select(PageAttachment::as_select())
        .load(connection)
        .await
}

/// Attachment `id` of page `page_id`, unless it was removed.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it holds no such attachment.
pub async fn find(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    id: Uuid,
) -> QueryResult<PageAttachment> {
    page_attachments::table
        .filter(page_attachments::id.eq(id))
        .filter(page_attachments::page_id.eq(page_id))
        .filter(page_attachments::deleted_at.is_null())
        .select(PageAttachment::as_select())
        .first(connection)
        .await
}

/// The location a new attachment on page `page_id` would be written to, once `author` is found to
/// be allowed to add one. Asked before a byte moves, so a refusal costs nothing, and [`add`] checks
/// the page again once the file is written.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::Deleted`] for a deleted one,
/// [`PageError::ProposalOnly`] for an agent on a page agents only propose to,
/// [`PageError::TooManyAttachments`] for a page already holding [`MAX_ATTACHMENTS_PER_PAGE`], and
/// [`PageError::NoAttachmentLocation`] until an admin chooses a location.
pub async fn destination_for(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    author: Author<'_>,
) -> Result<Uuid, PageError> {
    let page = page::find(connection, page_id).await?;
    page::require_writable(connection, &page, author).await?;
    require_room(connection, page_id).await?;
    user::workspace_settings(connection)
        .await?
        .page_attachment_storage_location_id
        .ok_or(PageError::NoAttachmentLocation)
}

/// Refuses another attachment on a page already holding as many as one may. Exact only while the
/// caller holds the page's row, which a refusal before the file moves does not need.
async fn require_room(connection: &mut AsyncPgConnection, page_id: Uuid) -> Result<(), PageError> {
    let held: i64 = page_attachments::table
        .filter(page_attachments::page_id.eq(page_id))
        .filter(page_attachments::deleted_at.is_null())
        .count()
        .get_result(connection)
        .await?;
    if held >= MAX_ATTACHMENTS_PER_PAGE {
        return Err(PageError::TooManyAttachments {
            max: MAX_ATTACHMENTS_PER_PAGE,
        });
    }
    Ok(())
}

/// Records `file` on its page and in the page's history.
///
/// # Errors
/// `NotFound` for an unknown page, and as [`destination_for`] refuses for one that changed while
/// the file was being written. The caller then owns a file nothing records.
pub async fn add(
    connection: &mut AsyncPgConnection,
    file: StoredFile<'_>,
    author: Author<'_>,
) -> Result<(PageAttachment, Recorded), PageError> {
    connection
        .transaction(async move |connection| {
            let page = page::lock(connection, file.page_id).await?;
            page::require_writable(connection, &page, author).await?;
            require_room(connection, file.page_id).await?;

            let attachment = diesel::insert_into(page_attachments::table)
                .values(AttachmentRow {
                    id: file.id,
                    page_id: file.page_id,
                    storage_location_id: file.storage_location_id,
                    storage_path: file.storage_path,
                    file_name: file.file_name,
                    content_type: file.content_type,
                    size_bytes: file.size_bytes,
                    created_by: author.user_id,
                    agent: author.agent.map(Agent::id),
                    agent_name: author.agent.and_then(Agent::name),
                    created_at: author.at,
                })
                .returning(PageAttachment::as_returning())
                .get_result(connection)
                .await?;

            let data = json!({ "attachmentId": attachment.id, "name": attachment.file_name });
            let entry = page::record(
                connection,
                file.page_id,
                None,
                HistoryKind::Attached,
                author,
                data,
            )
            .await?;
            Ok((attachment, Recorded::of(page, entry)))
        })
        .await
}

/// Takes attachment `id` off page `page_id` and records it in the page's history. Its file stays
/// with the provider, and its address stops answering. One already removed records nothing.
///
/// # Errors
/// `NotFound` for an unknown page or attachment, [`PageError::Deleted`] for a deleted page, and
/// [`PageError::ProposalOnly`] for an agent on a page agents only propose to.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = page::lock(connection, page_id).await?;
            page::require_writable(connection, &page, author).await?;
            let attachment = holding(connection, page_id, id).await?;
            if attachment.deleted_at.is_some() {
                return Ok(Recorded::unchanged(page));
            }

            diesel::update(page_attachments::table.find(id))
                .set(page_attachments::deleted_at.eq(author.at))
                .execute(connection)
                .await?;

            let data = json!({ "attachmentId": id, "name": attachment.file_name });
            let entry = page::record(
                connection,
                page_id,
                None,
                HistoryKind::Detached,
                author,
                data,
            )
            .await?;
            Ok(Recorded::of(page, entry))
        })
        .await
}

/// Puts a removed attachment back on page `page_id`, which is what undoes [`remove`], and records
/// it as being attached again. One still attached records nothing.
///
/// # Errors
/// As [`remove`], and [`PageError::TooManyAttachments`] for a page that filled up since.
pub async fn restore(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    id: Uuid,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = page::lock(connection, page_id).await?;
            page::require_writable(connection, &page, author).await?;
            let attachment = holding(connection, page_id, id).await?;
            if attachment.deleted_at.is_none() {
                return Ok(Recorded::unchanged(page));
            }
            require_room(connection, page_id).await?;

            diesel::update(page_attachments::table.find(id))
                .set(page_attachments::deleted_at.eq(None::<DateTime<Utc>>))
                .execute(connection)
                .await?;

            let data = json!({ "attachmentId": id, "name": attachment.file_name });
            let entry = page::record(
                connection,
                page_id,
                None,
                HistoryKind::Attached,
                author,
                data,
            )
            .await?;
            Ok(Recorded::of(page, entry))
        })
        .await
}

/// Attachment `id` of page `page_id`, removed or not. The caller holds the page's row.
async fn holding(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    id: Uuid,
) -> QueryResult<PageAttachment> {
    page_attachments::table
        .filter(page_attachments::id.eq(id))
        .filter(page_attachments::page_id.eq(page_id))
        .select(PageAttachment::as_select())
        .first(connection)
        .await
}
