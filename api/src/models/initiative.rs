// Copyright © 2026 Jalapeno Labs

//! Initiatives: goals that end, grouping action items and carrying the progress bar.
//!
//! Items join and leave initiatives through `crate::models::action_item`; this module
//! owns the initiatives themselves, their projects, and reading their members' spans for
//! progress (see `crate::action_items::progress`). Like items, initiatives are deleted
//! softly, record every write in their history, and lock their row while a write runs.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Map, json};
use uuid::Uuid;

use crate::action_items::progress::Membership;
use crate::action_items::{Actor, WorkError};
use crate::database::schema::{action_items, initiative_items, initiative_projects, initiatives};
use crate::models::action_item::ProjectFilter;
use crate::models::action_item_event::{self, Change, HistoryKind, Recorded, Subject, replaced};
use crate::models::project;

/// Where an initiative stands.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::InitiativeState"]
#[serde(rename_all = "kebab-case")]
pub enum InitiativeState {
    Active,
    Achieved,
    Abandoned,
}

/// A stored initiative.
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = initiatives, check_for_backend(diesel::pg::Pg))]
pub struct Initiative {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub target_at: Option<DateTime<Utc>>,
    pub state: InitiativeState,
    pub deleted_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new initiative. It starts `active`.
#[derive(Debug)]
pub struct NewInitiative {
    pub name: String,
    pub description: String,
    pub target_at: Option<DateTime<Utc>>,
    /// Sorted and unique.
    pub project_ids: Vec<Uuid>,
}

/// A partial update. `None` leaves a field untouched; `target_at: Some(None)` clears it.
#[derive(Debug, Default)]
pub struct InitiativeChanges {
    pub name: Option<String>,
    pub description: Option<String>,
    #[expect(
        clippy::option_option,
        reason = "absent, cleared, and a date are three distinct requests"
    )]
    pub target_at: Option<Option<DateTime<Utc>>>,
    pub state: Option<InitiativeState>,
}

impl InitiativeChanges {
    /// True when there is nothing to apply.
    pub const fn is_empty(&self) -> bool {
        self.name.is_none()
            && self.description.is_none()
            && self.target_at.is_none()
            && self.state.is_none()
    }
}

/// Which initiatives a list returns. The default is every one that is not deleted.
#[derive(Debug, Default)]
pub struct InitiativeFilter {
    /// Initiatives in any of these states; empty for every state.
    pub states: Vec<InitiativeState>,
    pub project: Option<ProjectFilter>,
    /// Deleted initiatives only, instead of the rest.
    pub deleted: bool,
}

#[derive(Insertable)]
#[diesel(table_name = initiatives)]
struct InitiativeRow {
    id: Uuid,
    name: String,
    description: String,
    target_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
}

#[derive(Default, AsChangeset)]
#[diesel(table_name = initiatives)]
struct InitiativeChangeset {
    name: Option<String>,
    description: Option<String>,
    #[expect(
        clippy::option_option,
        reason = "Diesel's changeset encoding: outer None skips, Some(None) writes NULL"
    )]
    target_at: Option<Option<DateTime<Utc>>>,
    state: Option<InitiativeState>,
}

#[derive(Insertable)]
#[diesel(table_name = initiative_projects)]
struct ProjectLinkRow {
    initiative_id: Uuid,
    project_id: Uuid,
}

async fn lock(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Initiative> {
    initiatives::table
        .find(id)
        .for_update()
        .select(Initiative::as_select())
        .first(connection)
        .await
}

/// [`lock`], refusing a deleted initiative.
async fn lock_live(connection: &mut AsyncPgConnection, id: Uuid) -> Result<Initiative, WorkError> {
    let initiative = lock(connection, id).await?;
    if initiative.deleted_at.is_some() {
        return Err(WorkError::Conflict(
            "the initiative is deleted; restore it first",
        ));
    }
    Ok(initiative)
}

/// One initiative by id, deleted or not.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Initiative> {
    initiatives::table
        .find(id)
        .select(Initiative::as_select())
        .first(connection)
        .await
}

/// The initiatives among `ids` that are not deleted, by name.
///
/// # Errors
/// Propagates any database error.
pub async fn find_live(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
) -> QueryResult<Vec<Initiative>> {
    initiatives::table
        .filter(initiatives::id.eq_any(ids))
        .filter(initiatives::deleted_at.is_null())
        .order((initiatives::name.asc(), initiatives::id.asc()))
        .select(Initiative::as_select())
        .load(connection)
        .await
}

/// The initiatives `filter` selects, by name.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    filter: &InitiativeFilter,
) -> QueryResult<Vec<Initiative>> {
    let mut query = initiatives::table.into_boxed();

    query = if filter.deleted {
        query.filter(initiatives::deleted_at.is_not_null())
    } else {
        query.filter(initiatives::deleted_at.is_null())
    };
    if !filter.states.is_empty() {
        query = query.filter(initiatives::state.eq_any(filter.states.clone()));
    }
    match filter.project {
        None => {}
        Some(ProjectFilter::Project(project_id)) => {
            let linked = initiative_projects::table
                .filter(initiative_projects::project_id.eq(project_id))
                .select(initiative_projects::initiative_id);
            query = query.filter(initiatives::id.eq_any(linked));
        }
        Some(ProjectFilter::Unassigned) => {
            let linked = initiative_projects::table.select(initiative_projects::initiative_id);
            query = query.filter(diesel::dsl::not(initiatives::id.eq_any(linked)));
        }
    }

    query
        .order((initiatives::name.asc(), initiatives::id.asc()))
        .select(Initiative::as_select())
        .load(connection)
        .await
}

/// The projects each of `initiative_ids` belongs to.
///
/// # Errors
/// Propagates any database error.
pub async fn project_ids(
    connection: &mut AsyncPgConnection,
    initiative_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<Uuid>>> {
    let links: Vec<(Uuid, Uuid)> = initiative_projects::table
        .filter(initiative_projects::initiative_id.eq_any(initiative_ids))
        .order((
            initiative_projects::initiative_id,
            initiative_projects::project_id,
        ))
        .select((
            initiative_projects::initiative_id,
            initiative_projects::project_id,
        ))
        .load(connection)
        .await?;

    let mut project_ids_by_initiative: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (initiative_id, project_id) in links {
        project_ids_by_initiative
            .entry(initiative_id)
            .or_default()
            .push(project_id);
    }
    Ok(project_ids_by_initiative)
}

/// Every span every item spent in each of `initiative_ids`, past and current, with the
/// moments that decide how it counts toward progress.
///
/// # Errors
/// Propagates any database error.
pub async fn memberships(
    connection: &mut AsyncPgConnection,
    initiative_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<Membership>>> {
    type SpanRow = (
        Uuid,
        DateTime<Utc>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
        Option<DateTime<Utc>>,
    );
    let spans: Vec<SpanRow> = initiative_items::table
        .inner_join(action_items::table)
        .filter(initiative_items::initiative_id.eq_any(initiative_ids))
        .select((
            initiative_items::initiative_id,
            initiative_items::joined_at,
            initiative_items::left_at,
            action_items::resolved_at,
            action_items::dismissed_at,
            action_items::deleted_at,
        ))
        .load(connection)
        .await?;

    let mut memberships_by_initiative: HashMap<Uuid, Vec<Membership>> = HashMap::new();
    for (initiative_id, joined_at, left_at, resolved_at, dismissed_at, deleted_at) in spans {
        memberships_by_initiative
            .entry(initiative_id)
            .or_default()
            .push(Membership {
                joined_at,
                left_at,
                resolved_at,
                dismissed_at,
                deleted_at,
            });
    }
    Ok(memberships_by_initiative)
}

/// Creates an initiative with a new `UUIDv7` id, in its projects.
///
/// # Errors
/// Propagates database errors, including a foreign key violation for a project that does
/// not exist.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_initiative: NewInitiative,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Initiative>, WorkError> {
    let id = Uuid::now_v7();
    let created = json!({
        "name": new_initiative.name,
        "description": new_initiative.description,
        "targetAt": new_initiative.target_at,
        "projectIds": new_initiative.project_ids,
    });
    let project_links: Vec<ProjectLinkRow> = new_initiative
        .project_ids
        .iter()
        .map(|&project_id| ProjectLinkRow {
            initiative_id: id,
            project_id,
        })
        .collect();
    let row = InitiativeRow {
        id,
        name: new_initiative.name,
        description: new_initiative.description,
        target_at: new_initiative.target_at,
        created_at: now,
    };

    connection
        .transaction(async move |connection| {
            let record = diesel::insert_into(initiatives::table)
                .values(row)
                .returning(Initiative::as_returning())
                .get_result(connection)
                .await?;
            diesel::insert_into(initiative_projects::table)
                .values(project_links)
                .execute(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Initiative(id),
                    kind: HistoryKind::Created,
                    actor,
                    data: created,
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Applies `changes` to a live initiative, recording the fields that actually changed.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id, [`WorkError::Conflict`]
/// for a deleted initiative, and any other database error.
pub async fn update(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    changes: InitiativeChanges,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Initiative>, WorkError> {
    connection
        .transaction(async move |connection| {
            let initiative = lock_live(connection, id).await?;
            let mut diff = Map::new();
            let mut changeset = InitiativeChangeset::default();

            if let Some(name) = changes.name
                && name != initiative.name
            {
                diff.insert("name".to_owned(), replaced(&initiative.name, &name));
                changeset.name = Some(name);
            }
            if let Some(description) = changes.description
                && description != initiative.description
            {
                diff.insert(
                    "description".to_owned(),
                    replaced(&initiative.description, &description),
                );
                changeset.description = Some(description);
            }
            if let Some(target_at) = changes.target_at
                && target_at != initiative.target_at
            {
                diff.insert(
                    "targetAt".to_owned(),
                    replaced(initiative.target_at, target_at),
                );
                changeset.target_at = Some(target_at);
            }
            if let Some(state) = changes.state
                && state != initiative.state
            {
                diff.insert("state".to_owned(), replaced(initiative.state, state));
                changeset.state = Some(state);
            }

            if diff.is_empty() {
                return Ok(Recorded {
                    record: initiative,
                    history: Vec::new(),
                });
            }

            let record = diesel::update(initiatives::table.find(id))
                .set(changeset)
                .returning(Initiative::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Initiative(id),
                    kind: HistoryKind::Updated,
                    actor,
                    data: json!({ "changes": diff }),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Deletes an initiative softly. Its items stay, and stay members, until it is restored.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id, [`WorkError::Conflict`]
/// when it is already deleted, and any other database error.
pub async fn soft_delete(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Initiative>, WorkError> {
    connection
        .transaction(async move |connection| {
            lock_live(connection, id).await?;
            let record = diesel::update(initiatives::table.find(id))
                .set(initiatives::deleted_at.eq(now))
                .returning(Initiative::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Initiative(id),
                    kind: HistoryKind::Deleted,
                    actor,
                    data: json!({}),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Brings a deleted initiative back as it was.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id, [`WorkError::Conflict`]
/// when it is not deleted, and any other database error.
pub async fn restore(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Initiative>, WorkError> {
    connection
        .transaction(async move |connection| {
            let initiative = lock(connection, id).await?;
            let Some(deleted_at) = initiative.deleted_at else {
                return Err(WorkError::Conflict("the initiative is not deleted"));
            };
            let record = diesel::update(initiatives::table.find(id))
                .set(initiatives::deleted_at.eq(None::<DateTime<Utc>>))
                .returning(Initiative::as_returning())
                .get_result(connection)
                .await?;
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Initiative(id),
                    kind: HistoryKind::Restored,
                    actor,
                    data: json!({ "deletedAt": deleted_at }),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record,
                history: vec![entry],
            })
        })
        .await
}

/// Adds a live initiative to a project. Adding it again changes nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown initiative or project,
/// [`WorkError::Conflict`] for a deleted initiative, and any other database error.
pub async fn add_project(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Initiative>, WorkError> {
    connection
        .transaction(async move |connection| {
            let initiative = lock_live(connection, id).await?;
            project::lock_shared(connection, project_id).await?;
            let inserted = diesel::insert_into(initiative_projects::table)
                .values(ProjectLinkRow {
                    initiative_id: id,
                    project_id,
                })
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
            if inserted == 0 {
                return Ok(Recorded {
                    record: initiative,
                    history: Vec::new(),
                });
            }
            let entry = action_item_event::record(
                connection,
                Change {
                    subject: Subject::Initiative(id),
                    kind: HistoryKind::ProjectAdded,
                    actor,
                    data: json!({ "projectId": project_id }),
                    at: now,
                },
            )
            .await?;
            Ok(Recorded {
                record: initiative,
                history: vec![entry],
            })
        })
        .await
}

/// Takes a live initiative out of a project. Removing it when it is not there changes
/// nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown initiative,
/// [`WorkError::Conflict`] for a deleted one, and any other database error.
pub async fn remove_project(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Initiative>, WorkError> {
    connection
        .transaction(async move |connection| {
            let initiative = lock_live(connection, id).await?;
            Ok(unlink_project(connection, initiative, project_id, actor, now).await?)
        })
        .await
}

/// Takes every initiative, deleted ones included, out of a project about to be deleted,
/// so each records the removal instead of losing it silently to the cascade. Call it inside
/// the transaction that deletes the project, after [`project::lock_for_delete`].
///
/// # Errors
/// Propagates any database error.
pub async fn remove_all_from_project(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> QueryResult<Vec<Recorded<Initiative>>> {
    let initiative_ids: Vec<Uuid> = initiative_projects::table
        .filter(initiative_projects::project_id.eq(project_id))
        .order(initiative_projects::initiative_id)
        .select(initiative_projects::initiative_id)
        .load(connection)
        .await?;
    let mut removed = Vec::new();
    for initiative_id in initiative_ids {
        let initiative = lock(connection, initiative_id).await?;
        removed.push(unlink_project(connection, initiative, project_id, actor, now).await?);
    }
    Ok(removed)
}

/// Deletes the initiative's link to the project and records it, if there was one.
async fn unlink_project(
    connection: &mut AsyncPgConnection,
    initiative: Initiative,
    project_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> QueryResult<Recorded<Initiative>> {
    let removed = diesel::delete(
        initiative_projects::table
            .filter(initiative_projects::initiative_id.eq(initiative.id))
            .filter(initiative_projects::project_id.eq(project_id)),
    )
    .execute(connection)
    .await?;
    if removed == 0 {
        return Ok(Recorded {
            record: initiative,
            history: Vec::new(),
        });
    }
    let entry = action_item_event::record(
        connection,
        Change {
            subject: Subject::Initiative(initiative.id),
            kind: HistoryKind::ProjectRemoved,
            actor,
            data: json!({ "projectId": project_id }),
            at: now,
        },
    )
    .await?;
    Ok(Recorded {
        record: initiative,
        history: vec![entry],
    })
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::action_items::Transition;
    use crate::action_items::progress::{self, Progress};
    use crate::models::action_item::{
        self, ActionItemChanges, ActionItemFilter, ActionItemPriority, ActionItemState,
        NewActionItem, Owner,
    };
    use crate::models::action_item_event::list_for_initiative;
    use crate::models::project::NewProject;
    use crate::test_support::migrated_database;

    fn day(offset: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-01T09:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
            + TimeDelta::days(offset)
    }

    fn new_initiative(name: &str) -> NewInitiative {
        NewInitiative {
            name: name.to_owned(),
            description: String::new(),
            target_at: None,
            project_ids: Vec::new(),
        }
    }

    async fn create_initiative(connection: &mut AsyncPgConnection, name: &str) -> Initiative {
        create(connection, new_initiative(name), Actor::User, day(0))
            .await
            .expect("initiative")
            .record
    }

    /// An open item of the user's, in `initiative_ids` from `at`.
    async fn create_item(
        connection: &mut AsyncPgConnection,
        title: &str,
        initiative_ids: Vec<Uuid>,
        at: DateTime<Utc>,
    ) -> Uuid {
        let new_item = NewActionItem {
            title: title.to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids,
        };
        action_item::create(connection, new_item, Actor::User, at)
            .await
            .expect("item")
            .record
            .id
    }

    async fn progress_of(
        connection: &mut AsyncPgConnection,
        initiative_id: Uuid,
        at: DateTime<Utc>,
    ) -> Progress {
        let spans = memberships(connection, &[initiative_id])
            .await
            .expect("spans")
            .remove(&initiative_id)
            .unwrap_or_default();
        progress::at(&spans, at)
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn items_in_two_initiatives_count_toward_both_and_dropped_items_toward_neither() {
        let (_url, mut connection) = migrated_database().await;
        let storage = create_initiative(&mut connection, "Storage page").await;
        let deploy = create_initiative(&mut connection, "Deploy").await;
        let both = vec![deploy.id, storage.id];

        let shared_done = create_item(&mut connection, "shared, done", both.clone(), day(1)).await;
        create_item(&mut connection, "shared, open", both, day(1)).await;
        let dismissed = create_item(
            &mut connection,
            "storage, dismissed",
            vec![storage.id],
            day(1),
        )
        .await;
        let deleted =
            create_item(&mut connection, "deploy, deleted", vec![deploy.id], day(2)).await;
        let left = create_item(&mut connection, "deploy, left", vec![deploy.id], day(2)).await;
        // Someone else's work counts toward progress though it never reaches Next.
        let theirs =
            create_item(&mut connection, "storage, theirs", vec![storage.id], day(2)).await;
        let sam = ActionItemChanges {
            owner: Some(Owner::Other {
                name: "Sam".to_owned(),
            }),
            ..ActionItemChanges::default()
        };
        action_item::update(&mut connection, theirs, sam, Actor::User, day(2))
            .await
            .expect("hand to Sam");

        action_item::transition(
            &mut connection,
            shared_done,
            Transition::Resolve,
            Actor::User,
            day(3),
        )
        .await
        .expect("resolve");
        action_item::transition(
            &mut connection,
            dismissed,
            Transition::Dismiss,
            Actor::User,
            day(4),
        )
        .await
        .expect("dismiss");
        action_item::soft_delete(&mut connection, deleted, Actor::User, day(4))
            .await
            .expect("delete");
        action_item::leave_initiative(&mut connection, left, deploy.id, Actor::User, day(5))
            .await
            .expect("leave");

        assert_eq!(
            progress_of(&mut connection, storage.id, day(6)).await,
            Progress {
                resolved: 1,
                total: 3
            },
            "shared done, shared open, and theirs; the dismissed item is gone"
        );
        assert_eq!(
            progress_of(&mut connection, deploy.id, day(6)).await,
            Progress {
                resolved: 1,
                total: 2
            },
            "shared done and shared open; the deleted and departed items are gone"
        );

        let deploy_spans = memberships(&mut connection, &[deploy.id])
            .await
            .expect("spans")
            .remove(&deploy.id)
            .expect("deploy's spans");
        let burnup: Vec<(DateTime<Utc>, usize, usize)> =
            progress::burnup(&deploy_spans, deploy.created_at, day(6))
                .into_iter()
                .map(|point| (point.at, point.resolved, point.total))
                .collect();
        assert_eq!(
            burnup,
            [
                (day(0), 0, 0),
                (day(1), 0, 2),
                (day(2), 0, 4),
                (day(3), 1, 4),
                (day(4), 1, 3),
                (day(5), 1, 2),
                (day(6), 1, 2),
            ]
        );

        let restored = action_item::restore(&mut connection, deleted, Actor::User, day(7))
            .await
            .expect("restore");
        assert!(restored.record.deleted_at.is_none());
        assert_eq!(
            progress_of(&mut connection, deploy.id, day(7)).await.total,
            3,
            "a restored item counts again"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_deleted_initiative_leaves_lists_and_its_items_until_restored() {
        let (_url, mut connection) = migrated_database().await;
        let launch = create_initiative(&mut connection, "Launch").await;
        let item = create_item(&mut connection, "ship it", vec![launch.id], day(1)).await;

        soft_delete(&mut connection, launch.id, Actor::User, day(2))
            .await
            .expect("delete");
        let listed = list(&mut connection, &InitiativeFilter::default())
            .await
            .expect("list");
        assert!(listed.is_empty());
        let trash = InitiativeFilter {
            deleted: true,
            ..InitiativeFilter::default()
        };
        assert_eq!(list(&mut connection, &trash).await.expect("list").len(), 1);
        let item_memberships = action_item::memberships(&mut connection, &[item])
            .await
            .expect("memberships");
        assert!(
            item_memberships.initiative_ids(item).is_empty(),
            "items do not list a deleted initiative"
        );

        let rename = InitiativeChanges {
            name: Some("Relaunch".to_owned()),
            ..InitiativeChanges::default()
        };
        let refused = update(&mut connection, launch.id, rename, Actor::User, day(3)).await;
        assert!(matches!(refused, Err(WorkError::Conflict(_))));

        restore(&mut connection, launch.id, Actor::User, day(3))
            .await
            .expect("restore");
        let item_memberships = action_item::memberships(&mut connection, &[item])
            .await
            .expect("memberships");
        assert_eq!(item_memberships.initiative_ids(item), [launch.id]);
        let members = ActionItemFilter {
            initiative: Some(launch.id),
            ..ActionItemFilter::default()
        };
        let listed = action_item::list(&mut connection, &members, day(4))
            .await
            .expect("members");
        assert_eq!(listed.len(), 1, "membership survived the delete");

        let kinds: Vec<String> = list_for_initiative(&mut connection, launch.id)
            .await
            .expect("history")
            .into_iter()
            .map(|entry| entry.kind)
            .collect();
        assert_eq!(
            kinds,
            ["created", "initiative_joined", "deleted", "restored"]
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn updates_and_projects_record_their_changes() {
        let (_url, mut connection) = migrated_database().await;
        let project = project::create(
            &mut connection,
            &NewProject {
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project");
        let launch = create_initiative(&mut connection, "Launch").await;

        let achieve = InitiativeChanges {
            state: Some(InitiativeState::Achieved),
            target_at: Some(Some(day(30))),
            name: Some("Launch".to_owned()),
            ..InitiativeChanges::default()
        };
        let achieved = update(&mut connection, launch.id, achieve, Actor::User, day(1))
            .await
            .expect("update");
        assert_eq!(achieved.record.state, InitiativeState::Achieved);
        assert_eq!(
            achieved.history[0].data,
            json!({ "changes": {
                "state": { "from": "active", "to": "achieved" },
                "targetAt": { "from": null, "to": day(30) },
            } }),
            "the unchanged name is not recorded"
        );

        add_project(&mut connection, launch.id, project.id, Actor::User, day(2))
            .await
            .expect("add");
        let by_project = InitiativeFilter {
            project: Some(ProjectFilter::Project(project.id)),
            ..InitiativeFilter::default()
        };
        assert_eq!(
            list(&mut connection, &by_project)
                .await
                .expect("list")
                .len(),
            1
        );
        let unassigned = InitiativeFilter {
            project: Some(ProjectFilter::Unassigned),
            ..InitiativeFilter::default()
        };
        assert!(
            list(&mut connection, &unassigned)
                .await
                .expect("list")
                .is_empty()
        );
        let achieved_only = InitiativeFilter {
            states: vec![InitiativeState::Achieved],
            ..InitiativeFilter::default()
        };
        assert_eq!(
            list(&mut connection, &achieved_only)
                .await
                .expect("list")
                .len(),
            1
        );

        let removed = remove_project(&mut connection, launch.id, project.id, Actor::User, day(3))
            .await
            .expect("remove");
        assert_eq!(removed.history.len(), 1);
        assert!(
            project_ids(&mut connection, &[launch.id])
                .await
                .expect("projects")
                .is_empty()
        );
    }
}
