// Copyright © 2026 Jalapeno Labs

//! Containers linked to initiatives: a Jira epic or saved filter, a GitHub milestone or
//! label. The watcher keeps a container's children as the initiative's items, each linked
//! to its issue, so work tracked in the provider is never entered twice. See
//! `docs/action-items.md`.
//!
//! The membership a container brought is its own: `initiative_items.via_link_id` names it,
//! so a child that leaves the container leaves the initiative, and unlinking the container
//! takes out exactly the items it brought in.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::database::schema::{initiative_links, initiatives};
use crate::models::action_item::{self, ActionItem};
use crate::models::action_item_event::{self, Change, HistoryEntry, HistoryKind, Subject};
use crate::models::action_item_link::{LinkCredential, LinkProvider};
use crate::models::initiative;

/// What an initiative links to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ContainerKind"]
#[serde(rename_all = "kebab-case")]
pub enum ContainerKind {
    /// A Jira epic, or any issue with children; its children are the issues whose parent
    /// it is.
    Epic,
    /// A Jira saved filter; its children are the issues its JQL finds.
    Filter,
    /// A GitHub milestone; its children are the repository's issues in it.
    Milestone,
    /// A GitHub label on one repository; its children are the issues carrying it.
    Label,
}

/// The longest sync error a container keeps, matching its column.
const SYNC_ERROR_MAX_CHARACTERS: usize = 2000;

/// The longest title a container keeps, matching its column.
const TITLE_MAX_CHARACTERS: usize = 1000;

/// A stored container link.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = initiative_links, check_for_backend(diesel::pg::Pg))]
pub struct InitiativeLink {
    pub id: Uuid,
    pub initiative_id: Uuid,
    pub provider: LinkProvider,
    pub kind: ContainerKind,
    pub jira_credential_id: Option<Uuid>,
    pub github_credential_id: Option<Uuid>,
    pub external_id: String,
    pub external_key: String,
    pub url: String,
    pub title: String,
    /// When the watcher last read every child.
    pub synced_at: Option<DateTime<Utc>>,
    /// Why the latest read failed; `None` when it succeeded.
    pub sync_error: Option<String>,
    /// The container held more children than the watcher reads.
    pub truncated: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl InitiativeLink {
    pub fn credential(&self) -> LinkCredential {
        LinkCredential::from_columns(
            self.provider,
            self.jira_credential_id,
            self.github_credential_id,
        )
    }

    /// The container as history records it.
    pub fn summary(&self) -> Value {
        json!({
            "linkId": self.id,
            "provider": self.provider,
            "kind": self.kind,
            "key": self.external_key,
            "url": self.url,
        })
    }
}

/// Fields for a new container link, as the provider reported the container.
#[derive(Debug, Clone)]
pub struct NewContainer {
    pub credential: LinkCredential,
    pub kind: ContainerKind,
    pub external_id: String,
    pub external_key: String,
    pub url: String,
    pub title: String,
}

/// What unlinking a container did: the history it recorded and the items it took out.
#[derive(Debug)]
pub struct Unlinked {
    pub history: Vec<HistoryEntry>,
    pub items: Vec<ActionItem>,
}

#[derive(Insertable)]
#[diesel(table_name = initiative_links)]
struct ContainerRow {
    id: Uuid,
    initiative_id: Uuid,
    provider: LinkProvider,
    kind: ContainerKind,
    jira_credential_id: Option<Uuid>,
    github_credential_id: Option<Uuid>,
    external_id: String,
    external_key: String,
    url: String,
    title: String,
    created_at: DateTime<Utc>,
}

/// An initiative's containers, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_initiative(
    connection: &mut AsyncPgConnection,
    initiative_id: Uuid,
) -> QueryResult<Vec<InitiativeLink>> {
    initiative_links::table
        .filter(initiative_links::initiative_id.eq(initiative_id))
        .order((initiative_links::created_at, initiative_links::id))
        .select(InitiativeLink::as_select())
        .load(connection)
        .await
}

/// The containers of live initiatives that go through one credential, which the watcher
/// reads.
///
/// # Errors
/// Propagates any database error.
pub async fn watched(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
) -> QueryResult<Vec<InitiativeLink>> {
    let mut query = initiative_links::table
        .inner_join(initiatives::table)
        .filter(initiatives::deleted_at.is_null())
        .into_boxed();
    query = match credential.provider {
        LinkProvider::Jira => query.filter(initiative_links::jira_credential_id.eq(credential.id)),
        LinkProvider::Github => {
            query.filter(initiative_links::github_credential_id.eq(credential.id))
        }
    };
    query
        .order(initiative_links::id)
        .select(InitiativeLink::as_select())
        .load(connection)
        .await
}

/// Links a live initiative to a container. Linking the same container again changes
/// nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown initiative,
/// [`WorkError::Conflict`] for a deleted one, and any other database error.
pub async fn add(
    connection: &mut AsyncPgConnection,
    initiative_id: Uuid,
    container: NewContainer,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<(InitiativeLink, Vec<HistoryEntry>), WorkError> {
    connection
        .transaction(async move |connection| {
            initiative::lock_live(connection, initiative_id).await?;
            let (jira_credential_id, github_credential_id) = container.credential.columns();
            let inserted: Option<InitiativeLink> = diesel::insert_into(initiative_links::table)
                .values(ContainerRow {
                    id: Uuid::now_v7(),
                    initiative_id,
                    provider: container.credential.provider,
                    kind: container.kind,
                    jira_credential_id,
                    github_credential_id,
                    external_id: container.external_id.clone(),
                    external_key: container.external_key,
                    url: container.url,
                    title: container.title.chars().take(TITLE_MAX_CHARACTERS).collect(),
                    created_at: now,
                })
                .on_conflict_do_nothing()
                .returning(InitiativeLink::as_returning())
                .get_result(connection)
                .await
                .optional()?;

            let Some(link) = inserted else {
                let existing = list_for_initiative(connection, initiative_id)
                    .await?
                    .into_iter()
                    .find(|link| {
                        link.credential() == container.credential
                            && link.kind == container.kind
                            && link.external_id == container.external_id
                    })
                    .ok_or(WorkError::Conflict(
                        "the container could not be linked; try again",
                    ))?;
                return Ok((existing, Vec::new()));
            };

            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Initiative(initiative_id),
                    kind: HistoryKind::ContainerLinked,
                    actor,
                    data: link.summary(),
                    at: now,
                },
            )
            .await?;
            Ok((link, vec![entry]))
        })
        .await
}

/// Unlinks a container from a live initiative, taking out the items it brought in. The
/// items themselves stay, with their links.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown initiative or a container it
/// does not have, [`WorkError::Conflict`] for a deleted initiative, and any other database
/// error.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    initiative_id: Uuid,
    link_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Unlinked, WorkError> {
    connection
        .transaction(async move |connection| {
            initiative::lock_live(connection, initiative_id).await?;
            let link: InitiativeLink = initiative_links::table
                .filter(initiative_links::id.eq(link_id))
                .filter(initiative_links::initiative_id.eq(initiative_id))
                .select(InitiativeLink::as_select())
                .first(connection)
                .await?;

            let mut history = Vec::new();
            let mut items = Vec::new();
            for item_id in action_item::ids_via_container(connection, link.id).await? {
                let left = action_item::leave_initiative_via(
                    connection,
                    item_id,
                    initiative_id,
                    link.id,
                    actor,
                    now,
                )
                .await?;
                history.extend(left.history);
                items.push(left.record);
            }

            diesel::delete(initiative_links::table.find(link.id))
                .execute(connection)
                .await?;
            history.push(
                action_item_event::record(
                    connection,
                    Change {
                        subject: Subject::Initiative(initiative_id),
                        kind: HistoryKind::ContainerUnlinked,
                        actor,
                        data: link.summary(),
                        at: now,
                    },
                )
                .await?,
            );
            Ok(Unlinked { history, items })
        })
        .await
}

/// Records how the watcher's latest read of a container went: its title as the provider
/// names it now, whether it held more children than were read, and the error when it
/// failed. A failed read keeps the title it had.
///
/// # Errors
/// Propagates any database error.
pub async fn record_sync(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    outcome: Result<(&str, bool), &str>,
    now: DateTime<Utc>,
) -> QueryResult<InitiativeLink> {
    let update = diesel::update(initiative_links::table.find(id));
    match outcome {
        Ok((title, truncated)) => {
            let title: String = title.chars().take(TITLE_MAX_CHARACTERS).collect();
            update
                .set((
                    initiative_links::title.eq(title),
                    initiative_links::truncated.eq(truncated),
                    initiative_links::synced_at.eq(now),
                    initiative_links::sync_error.eq(None::<String>),
                ))
                .returning(InitiativeLink::as_returning())
                .get_result(connection)
                .await
        }
        Err(error) => {
            let error: String = error.chars().take(SYNC_ERROR_MAX_CHARACTERS).collect();
            update
                .set(initiative_links::sync_error.eq(error))
                .returning(InitiativeLink::as_returning())
                .get_result(connection)
                .await
        }
    }
}
