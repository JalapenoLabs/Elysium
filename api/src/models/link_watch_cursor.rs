// Copyright © 2026 Jalapeno Labs

//! How far the watcher has read each credential. Everything a provider updated before a
//! credential's `watched_through` has been applied, so a restart resumes from there
//! instead of reading every link again or missing what changed while Elysium was down.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::{
    action_item_links, action_items, initiative_links, initiatives, link_watch_cursors,
};
use crate::models::action_item_link::{LinkCredential, LinkProvider};

/// The longest error a cursor keeps, matching its column.
const ERROR_MAX_CHARACTERS: usize = 2000;

/// A stored cursor.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = link_watch_cursors, check_for_backend(diesel::pg::Pg))]
pub struct LinkWatchCursor {
    pub id: Uuid,
    pub jira_credential_id: Option<Uuid>,
    pub github_credential_id: Option<Uuid>,
    pub watched_through: DateTime<Utc>,
    /// Why the latest pass over the credential failed; `None` when it succeeded.
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = link_watch_cursors)]
struct CursorRow {
    id: Uuid,
    jira_credential_id: Option<Uuid>,
    github_credential_id: Option<Uuid>,
    watched_through: DateTime<Utc>,
}

/// Every credential that a live item's link or a live initiative's container goes through,
/// which is what the watcher reads.
///
/// # Errors
/// Propagates any database error.
pub async fn credentials_to_watch(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Vec<LinkCredential>> {
    type CredentialColumns = (LinkProvider, Option<Uuid>, Option<Uuid>);
    let from_items: Vec<CredentialColumns> = action_item_links::table
        .inner_join(action_items::table)
        .filter(action_items::deleted_at.is_null())
        .select((
            action_item_links::provider,
            action_item_links::jira_credential_id,
            action_item_links::github_credential_id,
        ))
        .distinct()
        .load(connection)
        .await?;
    let from_containers: Vec<CredentialColumns> = initiative_links::table
        .inner_join(initiatives::table)
        .filter(initiatives::deleted_at.is_null())
        .select((
            initiative_links::provider,
            initiative_links::jira_credential_id,
            initiative_links::github_credential_id,
        ))
        .distinct()
        .load(connection)
        .await?;

    let mut credentials: Vec<LinkCredential> = Vec::new();
    for (provider, jira_credential_id, github_credential_id) in
        from_items.into_iter().chain(from_containers)
    {
        let credential =
            LinkCredential::from_columns(provider, jira_credential_id, github_credential_id);
        if !credentials.contains(&credential) {
            credentials.push(credential);
        }
    }
    Ok(credentials)
}

/// The cursor of one credential, if the watcher has read it before.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
) -> QueryResult<Option<LinkWatchCursor>> {
    let query = link_watch_cursors::table.into_boxed();
    let query = match credential.provider {
        LinkProvider::Jira => {
            query.filter(link_watch_cursors::jira_credential_id.eq(credential.id))
        }
        LinkProvider::Github => {
            query.filter(link_watch_cursors::github_credential_id.eq(credential.id))
        }
    };
    query
        .select(LinkWatchCursor::as_select())
        .first(connection)
        .await
        .optional()
}

/// Moves a credential's cursor to `watched_through` after a pass that applied everything
/// up to it.
///
/// # Errors
/// Propagates any database error.
pub async fn advance(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
    watched_through: DateTime<Utc>,
) -> QueryResult<()> {
    if let Some(cursor) = find(connection, credential).await? {
        diesel::update(link_watch_cursors::table.find(cursor.id))
            .set((
                link_watch_cursors::watched_through.eq(watched_through),
                link_watch_cursors::last_error.eq(None::<String>),
            ))
            .execute(connection)
            .await?;
        return Ok(());
    }

    let (jira_credential_id, github_credential_id) = credential.columns();
    diesel::insert_into(link_watch_cursors::table)
        .values(CursorRow {
            id: Uuid::now_v7(),
            jira_credential_id,
            github_credential_id,
            watched_through,
        })
        .execute(connection)
        .await?;
    Ok(())
}

/// Records why a pass over a credential failed, leaving its cursor where it was so the
/// next pass reads the same span again.
///
/// # Errors
/// Propagates any database error.
pub async fn record_failure(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
    error: &str,
) -> QueryResult<()> {
    let error: String = error.chars().take(ERROR_MAX_CHARACTERS).collect();
    let Some(cursor) = find(connection, credential).await? else {
        // A credential never read has no cursor to keep the error on; the pass logs it.
        return Ok(());
    };
    diesel::update(link_watch_cursors::table.find(cursor.id))
        .set(link_watch_cursors::last_error.eq(error))
        .execute(connection)
        .await?;
    Ok(())
}
