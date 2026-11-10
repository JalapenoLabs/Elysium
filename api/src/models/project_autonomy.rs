// Copyright © 2026 Jalapeno Labs

//! The trust dial: each project's one level of autonomy, which says what of its Project Manager's
//! work waits for a person. See `docs/autonomy.md`, The dial.
//!
//! | Level                   | Batches that trace to a person | Its writes to trackers |
//! |-------------------------|--------------------------------|------------------------|
//! | Approve every batch     | Wait for approval              | Wait for approval      |
//! | Approve found work only | Run                            | Wait for approval      |
//! | Full autonomy           | Run                            | Apply at once          |
//!
//! [`Autonomy`]'s two rules are the only place a level's meaning is decided: submitting a batch
//! asks [`Autonomy::queues`] (`crate::models::work_batch::submit`), and a Project Manager's
//! changes ask [`Autonomy::writes_directly`] (`crate::tools::work::manager_changes`). A found
//! batch is proposed at every level, so no setting lets work an agent noticed queue itself.
//!
//! Only a person turns the dial: [`set`] takes a [`MemberUser`], which no machine user can be, so
//! no tool Elysia or a Project Manager holds reaches it. Each turn is a row of
//! `project_autonomy_changes`, with who made it and when, and the newest is what the project's page
//! shows ([`latest_change`]). A turn applies to what is submitted from then on; batches already
//! proposed or queued stay where they are, for the person to decide on the work queue.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::project::{self, Project};
use super::work_batch::lineage::Origin;
use crate::auth::MemberUser;
use crate::database::schema::{project_autonomy_changes, projects};

/// What of a project's work waits for a person.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    Hash,
    diesel_derive_enum::DbEnum,
    Serialize,
    Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::ProjectAutonomy"]
#[serde(rename_all = "snake_case")]
#[expect(
    clippy::enum_variant_names,
    reason = "each variant is named for its level as people read it: Full autonomy"
)]
pub enum Autonomy {
    /// Every batch waits for a person's approval, and so does every change to a tracker. Where
    /// every project starts: trust is earned.
    #[default]
    ApproveEveryBatch,
    /// A batch that traces to a person runs; a found one, and every change to a tracker, waits.
    ApproveFoundWork,
    /// A batch that traces to a person runs, and changes to trackers apply at once; a found
    /// batch still waits.
    FullAutonomy,
}

impl Autonomy {
    /// Whether a batch of `origin` goes straight to the queue rather than waiting for a person.
    /// Never for found work, whatever the level: work an agent noticed is a person's to accept.
    pub const fn queues(self, origin: Origin) -> bool {
        match origin {
            Origin::Found => false,
            Origin::HumanRooted => !matches!(self, Self::ApproveEveryBatch),
        }
    }

    /// Whether the Project Manager's changes to items and trackers apply as it makes them, rather
    /// than waiting as a changeset for a person's approval.
    pub const fn writes_directly(self) -> bool {
        matches!(self, Self::FullAutonomy)
    }
}

/// One turn of a project's dial.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = project_autonomy_changes, check_for_backend(diesel::pg::Pg))]
pub struct Change {
    pub id: Uuid,
    pub project_id: Uuid,
    pub previous_autonomy: Autonomy,
    pub autonomy: Autonomy,
    /// The person who turned it.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Insertable)]
#[diesel(table_name = project_autonomy_changes)]
struct NewChange {
    id: Uuid,
    project_id: Uuid,
    previous_autonomy: Autonomy,
    autonomy: Autonomy,
    created_by: Uuid,
}

/// `project_id`'s level, share-locked until the transaction ends, so a person turning the dial
/// waits for a submission reading it, and the submission is decided by the level it read.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] for an unknown project; any other database error.
pub async fn for_share(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Autonomy> {
    projects::table
        .find(project_id)
        .for_share()
        .select(projects::autonomy)
        .first(connection)
        .await
}

/// Sets `project_id`'s level to `autonomy` as `person`, recording the change with who made it, in
/// one transaction under the project's lock. Answers the project as it now stands; choosing the
/// level it already has changes and records nothing.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] for an unknown project; any other database error.
pub async fn set(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    autonomy: Autonomy,
    person: &MemberUser,
) -> QueryResult<Project> {
    connection
        .transaction(async |connection| {
            let previous: Autonomy = projects::table
                .find(project_id)
                .for_update()
                .select(projects::autonomy)
                .first(connection)
                .await?;
            if previous == autonomy {
                return project::find(connection, project_id).await;
            }

            diesel::insert_into(project_autonomy_changes::table)
                .values(NewChange {
                    id: Uuid::now_v7(),
                    project_id,
                    previous_autonomy: previous,
                    autonomy,
                    created_by: person.id(),
                })
                .execute(connection)
                .await?;
            diesel::update(projects::table.find(project_id))
                .set(projects::autonomy.eq(autonomy))
                .returning(Project::as_returning())
                .get_result(connection)
                .await
        })
        .await
}

/// `project_id`'s newest change: who set the level it has, and when. `None` when nobody has turned
/// its dial, so it is at the default it started with.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_change(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
) -> QueryResult<Option<Change>> {
    project_autonomy_changes::table
        .filter(project_autonomy_changes::project_id.eq(project_id))
        .order(project_autonomy_changes::id.desc())
        .select(Change::as_select())
        .first(connection)
        .await
        .optional()
}

#[cfg(test)]
mod tests;
