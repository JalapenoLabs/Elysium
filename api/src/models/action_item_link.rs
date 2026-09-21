// Copyright © 2026 Jalapeno Labs

//! Links from action items to the external things they stand for: a Jira issue, a GitHub
//! issue, or a GitHub pull request.
//!
//! A link is a pointer, not a copy. It records what the thing is (`external_id`), how a
//! person and a call name it (`external_key`, `url`), and what the provider last reported
//! about it (`observed_*`), so the watcher can act on a change rather than on a state that
//! has not moved. See `docs/action-items.md`.
//!
//! One link on an item is its primary: where comments are posted, and whose assignee owns
//! the item. The first link added becomes primary, and only the user moves it afterwards,
//! so a link an agent adds never changes whose list an item is on. Every write here records
//! its history on the item in the same transaction.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::database::schema::{action_item_links, action_items};
use crate::models::action_item::{
    self, ActionItem, ActionItemChanges, NewActionItem, Owner, OwnerKind,
};
use crate::models::action_item_event::{
    self, Change, HistoryEntry, HistoryKind, Recorded, Subject, replaced,
};

/// Which provider a link, a container, or a credential belongs to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::LinkProvider"]
#[serde(rename_all = "kebab-case")]
pub enum LinkProvider {
    Jira,
    Github,
}

impl LinkProvider {
    /// The name stored and sent to clients, and the one an actor carries.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Jira => "jira",
            Self::Github => "github",
        }
    }
}

/// What an item links to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::LinkKind"]
#[serde(rename_all = "kebab-case")]
pub enum LinkKind {
    Issue,
    /// A GitHub pull request. Elysium never merges or closes one.
    PullRequest,
}

/// A linked thing's state as its provider last reported it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::LinkState"]
#[serde(rename_all = "kebab-case")]
pub enum LinkState {
    /// Still to do, or a pull request still under review.
    Open,
    /// A Jira issue in a `done` status category, or a GitHub issue closed as completed.
    Done,
    /// A GitHub issue closed as not planned.
    NotPlanned,
    /// A pull request that merged.
    Merged,
    /// A pull request closed without merging.
    ClosedUnmerged,
}

/// The credential a link reaches its provider through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LinkCredential {
    pub provider: LinkProvider,
    pub id: Uuid,
}

impl LinkCredential {
    /// The `jira_credential_id` and `github_credential_id` columns for this credential.
    pub const fn columns(self) -> (Option<Uuid>, Option<Uuid>) {
        match self.provider {
            LinkProvider::Jira => (Some(self.id), None),
            LinkProvider::Github => (None, Some(self.id)),
        }
    }

    /// The credential the two columns name.
    ///
    /// # Panics
    /// Panics when the provider's column is empty, which the tables' `_credential` checks
    /// rule out.
    pub fn from_columns(
        provider: LinkProvider,
        jira_credential_id: Option<Uuid>,
        github_credential_id: Option<Uuid>,
    ) -> Self {
        let id = match provider {
            LinkProvider::Jira => jira_credential_id,
            LinkProvider::Github => github_credential_id,
        };
        Self {
            provider,
            id: id.expect("the _credential check sets the provider's credential column"),
        }
    }
}

/// A stored link.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = action_item_links, check_for_backend(diesel::pg::Pg))]
pub struct ActionItemLink {
    pub id: Uuid,
    pub action_item_id: Uuid,
    pub provider: LinkProvider,
    pub kind: LinkKind,
    pub jira_credential_id: Option<Uuid>,
    pub github_credential_id: Option<Uuid>,
    /// What the thing is, for good: Jira's issue id, or GitHub's `owner/name#number`.
    pub external_id: String,
    /// What a person reads and a call names: `ELY-12`, or `owner/name#12`.
    pub external_key: String,
    pub url: String,
    /// The title as last read.
    pub title: String,
    pub is_primary: bool,
    pub observed_state: LinkState,
    pub observed_owner_kind: OwnerKind,
    /// Set exactly when `observed_owner_kind` is `Other`.
    pub observed_owner_name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ActionItemLink {
    pub fn credential(&self) -> LinkCredential {
        LinkCredential::from_columns(
            self.provider,
            self.jira_credential_id,
            self.github_credential_id,
        )
    }

    /// Whose the thing was when last read, in the item's terms.
    ///
    /// # Panics
    /// Panics if an `Other` owner has no name, which `action_item_links_observed_owner_name_set`
    /// rules out.
    pub fn observed_owner(&self) -> Owner {
        match self.observed_owner_kind {
            OwnerKind::User => Owner::User,
            OwnerKind::Other => Owner::Other {
                name: self
                    .observed_owner_name
                    .clone()
                    .expect("action_item_links_observed_owner_name_set requires a name"),
            },
            OwnerKind::Nobody => Owner::Nobody,
        }
    }

    /// The link as history records it.
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

/// What a provider reports about a linked thing, in the terms a link records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    pub external_key: String,
    pub url: String,
    pub title: String,
    pub state: LinkState,
    /// Whose it is: the credential's own account is the user.
    pub owner: Owner,
}

/// Fields for a new link.
#[derive(Debug, Clone)]
pub struct NewLink {
    pub credential: LinkCredential,
    pub kind: LinkKind,
    pub external_id: String,
    pub observation: Observation,
}

/// An item as a link write left it, with the links whose rows the write changed.
#[derive(Debug)]
pub struct Linked {
    pub item: Recorded<ActionItem>,
    pub links: Vec<ActionItemLink>,
}

#[derive(Insertable)]
#[diesel(table_name = action_item_links)]
struct LinkRow {
    id: Uuid,
    action_item_id: Uuid,
    provider: LinkProvider,
    kind: LinkKind,
    jira_credential_id: Option<Uuid>,
    github_credential_id: Option<Uuid>,
    external_id: String,
    external_key: String,
    url: String,
    title: String,
    is_primary: bool,
    observed_state: LinkState,
    observed_owner_kind: OwnerKind,
    observed_owner_name: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(AsChangeset)]
#[diesel(table_name = action_item_links, treat_none_as_null = true)]
struct ObservationChangeset {
    external_key: String,
    url: String,
    title: String,
    observed_state: LinkState,
    observed_owner_kind: OwnerKind,
    observed_owner_name: Option<String>,
}

/// The longest title a link keeps, matching its column.
const TITLE_MAX_CHARACTERS: usize = 1000;

impl From<&Observation> for ObservationChangeset {
    fn from(observation: &Observation) -> Self {
        let (observed_owner_kind, observed_owner_name) = owner_columns(&observation.owner);
        Self {
            external_key: observation.external_key.clone(),
            url: observation.url.clone(),
            title: observation
                .title
                .chars()
                .take(TITLE_MAX_CHARACTERS)
                .collect(),
            observed_state: observation.state,
            observed_owner_kind,
            observed_owner_name,
        }
    }
}

/// The owner columns for `owner`.
fn owner_columns(owner: &Owner) -> (OwnerKind, Option<String>) {
    match owner {
        Owner::User => (OwnerKind::User, None),
        Owner::Other { name } => (OwnerKind::Other, Some(name.clone())),
        Owner::Nobody => (OwnerKind::Nobody, None),
    }
}

/// An item's links, the primary first, then oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Vec<ActionItemLink>> {
    action_item_links::table
        .filter(action_item_links::action_item_id.eq(action_item_id))
        .order((
            action_item_links::is_primary.desc(),
            action_item_links::created_at.asc(),
            action_item_links::id.asc(),
        ))
        .select(ActionItemLink::as_select())
        .load(connection)
        .await
}

/// One link by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no link has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<ActionItemLink> {
    action_item_links::table
        .find(id)
        .select(ActionItemLink::as_select())
        .first(connection)
        .await
}

/// One link of one item.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the item has no link with that id.
pub async fn find_for_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    link_id: Uuid,
) -> QueryResult<ActionItemLink> {
    action_item_links::table
        .filter(action_item_links::id.eq(link_id))
        .filter(action_item_links::action_item_id.eq(action_item_id))
        .select(ActionItemLink::as_select())
        .first(connection)
        .await
}

/// The link to one external thing through one credential, whichever item it is on.
///
/// # Errors
/// Propagates any database error.
pub async fn find_by_external(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
    kind: LinkKind,
    external_id: &str,
) -> QueryResult<Option<ActionItemLink>> {
    let mut query = action_item_links::table
        .filter(action_item_links::kind.eq(kind))
        .filter(action_item_links::external_id.eq(external_id))
        .into_boxed();
    query = match credential.provider {
        LinkProvider::Jira => query.filter(action_item_links::jira_credential_id.eq(credential.id)),
        LinkProvider::Github => {
            query.filter(action_item_links::github_credential_id.eq(credential.id))
        }
    };
    query
        .select(ActionItemLink::as_select())
        .first(connection)
        .await
        .optional()
}

/// The links of live items that go through one credential, which the watcher reads.
///
/// # Errors
/// Propagates any database error.
pub async fn watched(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
) -> QueryResult<Vec<ActionItemLink>> {
    let mut query = action_item_links::table
        .inner_join(action_items::table)
        .filter(action_items::deleted_at.is_null())
        .into_boxed();
    query = match credential.provider {
        LinkProvider::Jira => query.filter(action_item_links::jira_credential_id.eq(credential.id)),
        LinkProvider::Github => {
            query.filter(action_item_links::github_credential_id.eq(credential.id))
        }
    };
    query
        .order(action_item_links::id)
        .select(ActionItemLink::as_select())
        .load(connection)
        .await
}

/// When the oldest link through one credential was made, the furthest back the watcher
/// ever needs to read it.
///
/// # Errors
/// Propagates any database error.
pub async fn oldest_created_at(
    connection: &mut AsyncPgConnection,
    credential: LinkCredential,
) -> QueryResult<Option<DateTime<Utc>>> {
    let mut query = action_item_links::table.into_boxed();
    query = match credential.provider {
        LinkProvider::Jira => query.filter(action_item_links::jira_credential_id.eq(credential.id)),
        LinkProvider::Github => {
            query.filter(action_item_links::github_credential_id.eq(credential.id))
        }
    };
    query
        .select(diesel::dsl::min(action_item_links::created_at))
        .first(connection)
        .await
}

/// Records what the provider reports about a linked thing now.
///
/// # Errors
/// Propagates any database error.
pub async fn record_observation(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    observation: &Observation,
) -> QueryResult<ActionItemLink> {
    diesel::update(action_item_links::table.find(id))
        .set(ObservationChangeset::from(observation))
        .returning(ActionItemLink::as_returning())
        .get_result(connection)
        .await
}

/// Records a state Elysium itself put the thing in, such as done after a close landed, so
/// the watcher reads no change when the provider reports it.
///
/// # Errors
/// Propagates any database error.
pub async fn record_state(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    state: LinkState,
) -> QueryResult<ActionItemLink> {
    diesel::update(action_item_links::table.find(id))
        .set(action_item_links::observed_state.eq(state))
        .returning(ActionItemLink::as_returning())
        .get_result(connection)
        .await
}

/// Links a live item to an external thing. Linking the same thing to the same item again
/// changes nothing.
///
/// With `claims_primary`, the link becomes the primary of an item that has none, and the
/// item's owner then follows the thing's assignee: the user's first link does. An agent's
/// link never claims it, so linking the pull request it opened never moves whose list the
/// item is on.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`]
/// for a deleted item or a thing already linked to another item, and any other database
/// error.
pub async fn add(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    new_link: NewLink,
    claims_primary: bool,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Linked, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = action_item::lock_live(connection, action_item_id).await?;
            let existing = find_by_external(
                connection,
                new_link.credential,
                new_link.kind,
                &new_link.external_id,
            )
            .await?;
            if let Some(existing) = existing {
                if existing.action_item_id != action_item_id {
                    return Err(WorkError::Conflict(
                        "that is already linked to another action item",
                    ));
                }
                return Ok(Linked {
                    item: Recorded {
                        record: item,
                        history: Vec::new(),
                    },
                    links: Vec::new(),
                });
            }

            let has_primary = diesel::select(diesel::dsl::exists(
                action_item_links::table
                    .filter(action_item_links::action_item_id.eq(action_item_id))
                    .filter(action_item_links::is_primary),
            ))
            .get_result::<bool>(connection)
            .await?;
            let is_primary = claims_primary && !has_primary;
            let link = insert(connection, action_item_id, new_link, is_primary, now).await?;

            let mut history = vec![
                record_on_item(
                    connection,
                    action_item_id,
                    HistoryKind::LinkAdded,
                    actor,
                    link.summary(),
                    now,
                )
                .await?,
            ];
            let mut item = item;
            if link.is_primary {
                let followed = follow_owner(connection, &link, actor, now).await?;
                item = followed.record;
                history.extend(followed.history);
            }

            Ok(Linked {
                item: Recorded {
                    record: item,
                    history,
                },
                links: vec![link],
            })
        })
        .await
}

/// Creates an item for an external thing, with the link to it as its primary, in one
/// transaction. The item's owner is the caller's to set from the thing's assignee.
///
/// # Errors
/// Returns [`WorkError::Conflict`] for a thing already linked to an item, and otherwise as
/// [`action_item::create`].
pub async fn create_linked_item(
    connection: &mut AsyncPgConnection,
    new_item: NewActionItem,
    new_link: NewLink,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Linked, WorkError> {
    connection
        .transaction(async move |connection| {
            let existing = find_by_external(
                connection,
                new_link.credential,
                new_link.kind,
                &new_link.external_id,
            )
            .await?;
            if existing.is_some() {
                return Err(WorkError::Conflict(
                    "that is already linked to an action item",
                ));
            }
            let mut created = action_item::create(connection, new_item, actor, now).await?;
            let id = created.record.id;
            let link = insert(connection, id, new_link, true, now).await?;
            created.history.push(
                record_on_item(
                    connection,
                    id,
                    HistoryKind::LinkAdded,
                    actor,
                    link.summary(),
                    now,
                )
                .await?,
            );
            Ok(Linked {
                item: created,
                links: vec![link],
            })
        })
        .await
}

/// Inserts a link row. Used by [`add`] and [`create_linked_item`].
///
/// # Errors
/// Propagates any database error, including a unique violation for a thing already linked.
async fn insert(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    new_link: NewLink,
    is_primary: bool,
    now: DateTime<Utc>,
) -> QueryResult<ActionItemLink> {
    let (jira_credential_id, github_credential_id) = new_link.credential.columns();
    let observation = ObservationChangeset::from(&new_link.observation);
    diesel::insert_into(action_item_links::table)
        .values(LinkRow {
            id: Uuid::now_v7(),
            action_item_id,
            provider: new_link.credential.provider,
            kind: new_link.kind,
            jira_credential_id,
            github_credential_id,
            external_id: new_link.external_id,
            external_key: observation.external_key,
            url: observation.url,
            title: observation.title,
            is_primary,
            observed_state: observation.observed_state,
            observed_owner_kind: observation.observed_owner_kind,
            observed_owner_name: observation.observed_owner_name,
            created_at: now,
        })
        .returning(ActionItemLink::as_returning())
        .get_result(connection)
        .await
}

/// Unlinks a live item from one of its links. When that was the primary, the oldest link
/// left takes its place and the owner follows it; the user removed the primary, so the
/// user moved it.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item or a link it does not
/// have, [`WorkError::Conflict`] for a deleted item, and any other database error.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    link_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Linked, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = action_item::lock_live(connection, action_item_id).await?;
            let link = find_for_item(connection, action_item_id, link_id).await?;
            diesel::delete(action_item_links::table.find(link_id))
                .execute(connection)
                .await?;
            let mut history = vec![
                record_on_item(
                    connection,
                    action_item_id,
                    HistoryKind::LinkRemoved,
                    actor,
                    link.summary(),
                    now,
                )
                .await?,
            ];

            let mut item = item;
            let mut links = Vec::new();
            if link.is_primary {
                let next = list_for_item(connection, action_item_id)
                    .await?
                    .into_iter()
                    .next();
                if let Some(next) = next {
                    let promoted = set_primary(connection, next.id, true).await?;
                    let changed = promote(connection, &link, &promoted, actor, now).await?;
                    item = changed.record;
                    history.extend(changed.history);
                    links.push(promoted);
                } else {
                    history.push(
                        record_on_item(
                            connection,
                            action_item_id,
                            HistoryKind::PrimaryLinkChanged,
                            actor,
                            replaced(link.id, None::<Uuid>),
                            now,
                        )
                        .await?,
                    );
                }
            }

            Ok(Linked {
                item: Recorded {
                    record: item,
                    history,
                },
                links,
            })
        })
        .await
}

/// Makes one of a live item's links its primary, and the owner follows that link's
/// assignee. Choosing the primary again changes nothing.
///
/// # Errors
/// As [`remove`].
pub async fn make_primary(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    link_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Linked, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = action_item::lock_live(connection, action_item_id).await?;
            let chosen = find_for_item(connection, action_item_id, link_id).await?;
            if chosen.is_primary {
                return Ok(Linked {
                    item: Recorded {
                        record: item,
                        history: Vec::new(),
                    },
                    links: Vec::new(),
                });
            }

            let previous: Option<ActionItemLink> = action_item_links::table
                .filter(action_item_links::action_item_id.eq(action_item_id))
                .filter(action_item_links::is_primary)
                .select(ActionItemLink::as_select())
                .first(connection)
                .await
                .optional()?;
            let mut links = Vec::new();
            if let Some(previous) = &previous {
                links.push(set_primary(connection, previous.id, false).await?);
            }
            let promoted = set_primary(connection, link_id, true).await?;

            let history_from = previous.as_ref().map(|link| link.id);
            let mut history = vec![
                record_on_item(
                    connection,
                    action_item_id,
                    HistoryKind::PrimaryLinkChanged,
                    actor,
                    replaced(history_from, promoted.id),
                    now,
                )
                .await?,
            ];
            let followed = follow_owner(connection, &promoted, actor, now).await?;
            history.extend(followed.history);
            links.push(promoted);

            Ok(Linked {
                item: Recorded {
                    record: followed.record,
                    history,
                },
                links,
            })
        })
        .await
}

/// Records the move of the primary from a removed link to `promoted`, and follows its owner.
async fn promote(
    connection: &mut AsyncPgConnection,
    removed: &ActionItemLink,
    promoted: &ActionItemLink,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    let entry = record_on_item(
        connection,
        promoted.action_item_id,
        HistoryKind::PrimaryLinkChanged,
        actor,
        replaced(removed.id, promoted.id),
        now,
    )
    .await?;
    let mut followed = follow_owner(connection, promoted, actor, now).await?;
    followed.history.insert(0, entry);
    Ok(followed)
}

async fn set_primary(
    connection: &mut AsyncPgConnection,
    link_id: Uuid,
    is_primary: bool,
) -> QueryResult<ActionItemLink> {
    diesel::update(action_item_links::table.find(link_id))
        .set(action_item_links::is_primary.eq(is_primary))
        .returning(ActionItemLink::as_returning())
        .get_result(connection)
        .await
}

/// Makes the item's owner the primary link's observed owner. Changes nothing when they
/// already agree.
///
/// # Errors
/// Propagates [`action_item::update`]'s errors.
pub async fn follow_owner(
    connection: &mut AsyncPgConnection,
    primary: &ActionItemLink,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    let changes = ActionItemChanges {
        owner: Some(primary.observed_owner()),
        ..ActionItemChanges::default()
    };
    action_item::update(connection, primary.action_item_id, changes, actor, now).await
}

/// Records one change about an item.
///
/// # Errors
/// Propagates any database error.
pub async fn record_on_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    kind: HistoryKind,
    actor: Actor,
    data: Value,
    at: DateTime<Utc>,
) -> QueryResult<HistoryEntry> {
    action_item_event::record(
        connection,
        Change {
            subject: Subject::Item(action_item_id),
            kind,
            actor,
            data,
            at,
        },
    )
    .await
}
