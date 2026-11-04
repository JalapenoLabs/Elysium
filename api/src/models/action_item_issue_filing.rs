// Copyright © 2026 Jalapeno Labs

//! Issues Elysium owes a provider: an item a changeset asked to have its own issue in a
//! tracker, which the watcher has not filed yet.
//!
//! A filing is owed in the same transaction as the change that owes it (applying a changeset's
//! `file-issue` operation), so a crash between the change and the call never loses it, and the
//! watcher lands it on its next pass (`crate::action_items::watcher`), as it lands closes and
//! comments (`crate::models::action_item_link_write`). An item is filed once at a time: the
//! table holds one filing per item.
//!
//! Filing is the one write whose retry could duplicate what it made, since a second issue looks
//! like a first. So every attempt is recorded as it begins ([`begin_attempt`]), and an attempt
//! after the first asks the provider for the issue an earlier one may have opened before its
//! answer was lost (`crate::action_items::links::IssueToFile::tried_since`). Landing links that
//! issue to the item as its primary when it has none, and forgets the filing, in one
//! transaction ([`land`]). See `docs/action-items.md`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::action_items::changesets::Proposer;
use crate::action_items::{Actor, WorkError};
use crate::database::schema::{action_item_issue_filings, changeset_operations, timeline_records};
use crate::models::action_item::{self, ActionItem, ActionItemState};
use crate::models::action_item_event::{HistoryKind, Recorded};
use crate::models::action_item_link::{
    self, Claim, CredentialColumns, LinkCredential, LinkKind, LinkProvider, Linked, NewLink,
    Observation,
};

/// The longest provider answer a filing keeps, matching its column.
const ERROR_MAX_CHARACTERS: usize = 2000;

/// A filing that has not landed.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = action_item_issue_filings, check_for_backend(diesel::pg::Pg))]
pub struct IssueFiling {
    pub id: Uuid,
    pub action_item_id: Uuid,
    pub provider: LinkProvider,
    pub jira_credential_id: Option<Uuid>,
    pub github_credential_id: Option<Uuid>,
    pub token_account_id: Option<Uuid>,
    /// Where the issue opens: a Jira project key, a GitHub `owner/name`, or a YouTrack
    /// project's short name.
    pub destination: String,
    /// Who proposed it, in a changeset proposer's stored form.
    pub filed_by: String,
    pub changeset_operation_id: Option<Uuid>,
    /// The timeline record of the write, which links to the issue once it is filed.
    pub timeline_record_id: Option<Uuid>,
    pub attempts: i32,
    /// When the first attempt began; `None` until one has.
    pub first_attempt_at: Option<DateTime<Utc>>,
    /// The provider's last answer, or why the filing waits.
    pub last_error: Option<String>,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl IssueFiling {
    pub fn credential(&self) -> LinkCredential {
        LinkCredential::from_columns(
            self.provider,
            CredentialColumns {
                jira_credential_id: self.jira_credential_id,
                github_credential_id: self.github_credential_id,
                token_account_id: self.token_account_id,
            },
        )
    }

    /// Who the link it lands as is recorded as: its proposer.
    ///
    /// # Panics
    /// Panics if `filed_by` is not a proposer, which `action_item_issue_filings_filed_by_shape`
    /// rules out.
    pub fn actor(&self) -> Actor {
        Proposer::parse(&self.filed_by)
            .expect("action_item_issue_filings_filed_by_shape admits only proposers")
            .actor()
    }
}

/// A filing to owe.
#[derive(Debug, Clone)]
pub struct NewFiling {
    pub action_item_id: Uuid,
    pub credential: LinkCredential,
    pub destination: String,
    pub filed_by: Proposer,
    pub changeset_operation_id: Option<Uuid>,
    pub timeline_record_id: Option<Uuid>,
}

#[derive(Insertable)]
#[diesel(table_name = action_item_issue_filings)]
struct FilingRow {
    id: Uuid,
    action_item_id: Uuid,
    provider: LinkProvider,
    jira_credential_id: Option<Uuid>,
    github_credential_id: Option<Uuid>,
    token_account_id: Option<Uuid>,
    destination: String,
    filed_by: String,
    changeset_operation_id: Option<Uuid>,
    timeline_record_id: Option<Uuid>,
    created_at: DateTime<Utc>,
}

/// Owes a filing. Call it inside the transaction that asks for the issue.
///
/// # Errors
/// Returns [`WorkError::Refused`] for an item already being filed, and any other database
/// error.
pub async fn owe(
    connection: &mut AsyncPgConnection,
    new_filing: NewFiling,
    now: DateTime<Utc>,
) -> Result<IssueFiling, WorkError> {
    let columns = new_filing.credential.columns();
    let inserted = diesel::insert_into(action_item_issue_filings::table)
        .values(FilingRow {
            id: Uuid::now_v7(),
            action_item_id: new_filing.action_item_id,
            provider: new_filing.credential.provider,
            jira_credential_id: columns.jira_credential_id,
            github_credential_id: columns.github_credential_id,
            token_account_id: columns.token_account_id,
            destination: new_filing.destination,
            filed_by: new_filing.filed_by.actor().to_string(),
            changeset_operation_id: new_filing.changeset_operation_id,
            timeline_record_id: new_filing.timeline_record_id,
            created_at: now,
        })
        .on_conflict(action_item_issue_filings::action_item_id)
        .do_nothing()
        .returning(IssueFiling::as_returning())
        .get_result(connection)
        .await
        .optional()?;
    inserted.ok_or_else(|| WorkError::Refused("the item's issue is already being filed".to_owned()))
}

/// The filing an item owes, if it owes one.
///
/// # Errors
/// Propagates any database error.
pub async fn for_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Option<IssueFiling>> {
    action_item_issue_filings::table
        .filter(action_item_issue_filings::action_item_id.eq(action_item_id))
        .select(IssueFiling::as_select())
        .first(connection)
        .await
        .optional()
}

/// The filings the items `action_item_ids` owe.
///
/// # Errors
/// Propagates any database error.
pub async fn for_items(
    connection: &mut AsyncPgConnection,
    action_item_ids: &[Uuid],
) -> QueryResult<Vec<IssueFiling>> {
    action_item_issue_filings::table
        .filter(action_item_issue_filings::action_item_id.eq_any(action_item_ids))
        .order(action_item_issue_filings::id)
        .select(IssueFiling::as_select())
        .load(connection)
        .await
}

/// Up to `limit` owed filings: never-tried ones first, then the ones tried longest ago, so a
/// filing that keeps failing never holds back a new one.
///
/// # Errors
/// Propagates any database error.
pub async fn owed(connection: &mut AsyncPgConnection, limit: i64) -> QueryResult<Vec<IssueFiling>> {
    action_item_issue_filings::table
        .order((
            action_item_issue_filings::last_attempt_at
                .asc()
                .nulls_first(),
            action_item_issue_filings::id,
        ))
        .limit(limit)
        .select(IssueFiling::as_select())
        .load(connection)
        .await
}

/// Whether the item a filing is for still wants its issue: live, and in the inbox or open. A
/// resolved, dismissed, or deleted item's issue would be work nobody owes.
///
/// # Errors
/// Propagates any database error.
pub async fn still_wanted(
    connection: &mut AsyncPgConnection,
    filing: &IssueFiling,
) -> QueryResult<bool> {
    let item = action_item::find(connection, filing.action_item_id).await?;
    Ok(item.deleted_at.is_none()
        && matches!(item.state, ActionItemState::Inbox | ActionItemState::Open))
}

/// Records that an attempt at `filing` begins, before any call, and answers the filing as it
/// now stands. Committed on its own, so an attempt cut short is still known to have begun. One
/// watcher lands filings, so nothing else moves the attempt between reading and writing it.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for a filing that landed or was cancelled
/// meanwhile, and any other database error.
pub async fn begin_attempt(
    connection: &mut AsyncPgConnection,
    filing: &IssueFiling,
    now: DateTime<Utc>,
) -> QueryResult<IssueFiling> {
    diesel::update(action_item_issue_filings::table.find(filing.id))
        .set((
            action_item_issue_filings::attempts.eq(action_item_issue_filings::attempts + 1),
            action_item_issue_filings::first_attempt_at.eq(filing.first_attempt_at.or(Some(now))),
            action_item_issue_filings::last_attempt_at.eq(now),
        ))
        .returning(IssueFiling::as_returning())
        .get_result(connection)
        .await
}

/// Points a filing at the timeline record of the write that owed it, which links to the issue
/// once it is filed.
///
/// # Errors
/// Propagates any database error.
pub async fn set_timeline_record(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    timeline_record_id: Uuid,
) -> QueryResult<()> {
    diesel::update(action_item_issue_filings::table.find(id))
        .set(action_item_issue_filings::timeline_record_id.eq(timeline_record_id))
        .execute(connection)
        .await?;
    Ok(())
}

/// Records why an attempt did not land: the provider's answer, or why the filing waits.
///
/// # Errors
/// Propagates any database error.
pub async fn record_failure(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    message: &str,
) -> QueryResult<()> {
    let message: String = message.chars().take(ERROR_MAX_CHARACTERS).collect();
    diesel::update(action_item_issue_filings::table.find(id))
        .set(action_item_issue_filings::last_error.eq(message))
        .execute(connection)
        .await?;
    Ok(())
}

/// Forgets a filing that no longer applies, or that a person or an undo withdrew.
///
/// # Errors
/// Propagates any database error.
pub async fn forget(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    diesel::delete(action_item_issue_filings::table.find(id))
        .execute(connection)
        .await?;
    Ok(())
}

/// The user gives up on the issue an item is owed before it is filed, which the item's history
/// records.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the item owes no issue,
/// [`WorkError::Conflict`] for a deleted item, and any other database error.
pub async fn cancel(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = action_item::lock_live(connection, action_item_id).await?;
            let filing = for_item(connection, action_item_id)
                .await?
                .ok_or(diesel::result::Error::NotFound)?;
            forget(connection, filing.id).await?;
            let entry = action_item_link::record_on_item(
                connection,
                action_item_id,
                HistoryKind::IssueFilingCancelled,
                actor,
                json!({
                    "filingId": filing.id,
                    "provider": filing.provider,
                    "destination": filing.destination,
                }),
                now,
            )
            .await?;
            Ok(Recorded {
                record: item,
                history: vec![entry],
            })
        })
        .await
}

/// The issue a filing opened, as its link records it.
#[derive(Debug, Clone)]
pub struct Filed {
    pub external_id: String,
    pub observation: Observation,
}

/// Lands a filing: links the issue it opened to its item, as the item's primary when it has
/// none, points the write's timeline record at the issue, tells the changeset operation that
/// owed it which link it made, for an undo, and forgets the filing, in one transaction.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for a filing that is gone,
/// [`WorkError::Conflict`] for an item deleted meanwhile or an issue already linked to another
/// item, and any other database error.
pub async fn land(
    connection: &mut AsyncPgConnection,
    filing: &IssueFiling,
    filed: Filed,
    now: DateTime<Utc>,
) -> Result<Linked, WorkError> {
    let filing = filing.clone();
    connection
        .transaction(async move |connection| {
            // Held until the transaction ends, so a cancel or an undo in the same moment waits.
            let filing: IssueFiling = action_item_issue_filings::table
                .find(filing.id)
                .for_update()
                .select(IssueFiling::as_select())
                .first(connection)
                .await?;
            let key = filed.observation.external_key.clone();
            let url = filed.observation.url.clone();
            let new_link = NewLink {
                credential: filing.credential(),
                kind: LinkKind::Issue,
                external_id: filed.external_id,
                observation: filed.observation,
            };
            // The issue is the item's own, opened for it, so it is the primary whoever filed it,
            // and the item stays whose it was: nobody is assigned a new issue yet.
            let linked = action_item_link::add(
                connection,
                filing.action_item_id,
                new_link,
                Claim::PrimaryKeepingOwner,
                filing.actor(),
                now,
            )
            .await?;
            if let Some(record_id) = filing.timeline_record_id {
                let mut metadata: Value = timeline_records::table
                    .find(record_id)
                    .select(timeline_records::metadata)
                    .first(connection)
                    .await?;
                metadata["issueKey"] = json!(key);
                diesel::update(timeline_records::table.find(record_id))
                    .set((
                        timeline_records::url.eq(&url),
                        timeline_records::metadata.eq(metadata),
                    ))
                    .execute(connection)
                    .await?;
            }
            if let Some(operation_id) = filing.changeset_operation_id
                && let Some(link) = linked.links.as_slice().first()
            {
                let mut result: Value = changeset_operations::table
                    .find(operation_id)
                    .select(changeset_operations::result)
                    .first(connection)
                    .await?;
                result["linkId"] = json!(link.id);
                result["added"] = json!(!linked.item.history.is_empty());
                diesel::update(changeset_operations::table.find(operation_id))
                    .set(changeset_operations::result.eq(result))
                    .execute(connection)
                    .await?;
            }
            forget(connection, filing.id).await?;
            Ok(linked)
        })
        .await
}
