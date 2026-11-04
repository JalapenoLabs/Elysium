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
use crate::models::action_item_link::{CredentialColumns, LinkCredential, LinkProvider};

/// The longest error a cursor keeps, matching its column.
const ERROR_MAX_CHARACTERS: usize = 2000;

/// A stored cursor.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = link_watch_cursors, check_for_backend(diesel::pg::Pg))]
pub struct LinkWatchCursor {
    pub id: Uuid,
    pub jira_credential_id: Option<Uuid>,
    pub github_credential_id: Option<Uuid>,
    pub token_account_id: Option<Uuid>,
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
    token_account_id: Option<Uuid>,
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
    type Columns = (LinkProvider, Option<Uuid>, Option<Uuid>, Option<Uuid>);
    let from_items: Vec<Columns> = action_item_links::table
        .inner_join(action_items::table)
        .filter(action_items::deleted_at.is_null())
        .select((
            action_item_links::provider,
            action_item_links::jira_credential_id,
            action_item_links::github_credential_id,
            action_item_links::token_account_id,
        ))
        .distinct()
        .load(connection)
        .await?;
    let from_containers: Vec<(LinkProvider, Option<Uuid>, Option<Uuid>)> = initiative_links::table
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
    // Containers never go through a token account, so theirs is always empty.
    let from_containers = from_containers
        .into_iter()
        .map(|(provider, jira, github)| (provider, jira, github, None));

    let mut credentials: Vec<LinkCredential> = Vec::new();
    for (provider, jira_credential_id, github_credential_id, token_account_id) in
        from_items.into_iter().chain(from_containers)
    {
        let columns = CredentialColumns {
            jira_credential_id,
            github_credential_id,
            token_account_id,
        };
        let credential = LinkCredential::from_columns(provider, columns);
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
        LinkProvider::Youtrack => {
            query.filter(link_watch_cursors::token_account_id.eq(credential.id))
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

    let columns = credential.columns();
    diesel::insert_into(link_watch_cursors::table)
        .values(CursorRow {
            id: Uuid::now_v7(),
            jira_credential_id: columns.jira_credential_id,
            github_credential_id: columns.github_credential_id,
            token_account_id: columns.token_account_id,
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
