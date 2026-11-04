// Copyright © 2026 Jalapeno Labs

//! One change applied to many initiatives at once, as a person's multi-select asks.
//!
//! The same shape as `crate::models::action_item_bulk`: every initiative goes through the
//! write its single-initiative route uses ([`initiative::update`], [`initiative::soft_delete`],
//! [`initiative::restore`]), in one transaction with a savepoint for each. An initiative the
//! records refuse rolls back alone and is reported with the reason; a fault rolls back every
//! one. Initiatives are written in the order of their ids, so overlapping changes never
//! deadlock.

use chrono::{DateTime, Utc};
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::models::action_item_event::Recorded;
use crate::models::initiative::{self, Initiative, InitiativeChanges, InitiativeState};

/// The most initiatives one change names, as for items (`action_item_bulk::MAX_ITEMS`):
/// it keeps the transaction and the events it publishes short.
pub const MAX_INITIATIVES: usize = 1000;

/// What to do to every initiative named. Each is a write a single-initiative route makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BulkInitiativeChange {
    /// Any state from any state, as `PATCH` allows. One already there records nothing.
    State { state: InitiativeState },
    /// Deletes softly, so every initiative can be restored; its items stay members.
    #[expect(
        clippy::empty_enum_variants_with_brackets,
        reason = "serde refuses unknown fields only on a struct variant, as on every other body"
    )]
    Delete {},
    #[expect(
        clippy::empty_enum_variants_with_brackets,
        reason = "serde refuses unknown fields only on a struct variant, as on every other body"
    )]
    Restore {},
}

/// An initiative the change did not apply to, and why, in words for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InitiativeRefusal {
    pub initiative_id: Uuid,
    pub message: String,
}

/// What a change did.
#[derive(Debug, Default)]
pub struct InitiativeBulkOutcome {
    /// Every initiative the change applied to, as it stands now, with the history it
    /// recorded: none for one that already held what was asked.
    pub written: Vec<Recorded<Initiative>>,
    pub refused: Vec<InitiativeRefusal>,
}

/// Applies `change` to every initiative in `initiative_ids`, which must be sorted and free
/// of repeats.
///
/// # Errors
/// Returns any database fault, which writes nothing. An initiative the records refuse is
/// reported in [`InitiativeBulkOutcome::refused`].
pub async fn change_many(
    connection: &mut AsyncPgConnection,
    initiative_ids: &[Uuid],
    change: BulkInitiativeChange,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<InitiativeBulkOutcome, WorkError> {
    connection
        .transaction(async move |connection| {
            let mut outcome = InitiativeBulkOutcome::default();
            for &initiative_id in initiative_ids {
                let attempt = connection
                    .transaction(async move |connection| {
                        change_one(connection, initiative_id, change, actor, now).await
                    })
                    .await;
                match attempt {
                    Ok(written) => outcome.written.push(written),
                    Err(error) => outcome.refused.push(InitiativeRefusal {
                        initiative_id,
                        message: refusal_message(error)?,
                    }),
                }
            }
            Ok(outcome)
        })
        .await
}

/// Applies the change to one initiative through the write its single route makes.
async fn change_one(
    connection: &mut AsyncPgConnection,
    initiative_id: Uuid,
    change: BulkInitiativeChange,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<Initiative>, WorkError> {
    match change {
        BulkInitiativeChange::State { state } => {
            let changes = InitiativeChanges {
                state: Some(state),
                ..InitiativeChanges::default()
            };
            initiative::update(connection, initiative_id, changes, actor, now).await
        }
        BulkInitiativeChange::Delete {} => {
            initiative::soft_delete(connection, initiative_id, actor, now).await
        }
        BulkInitiativeChange::Restore {} => {
            initiative::restore(connection, initiative_id, actor, now).await
        }
    }
}

/// Why an initiative refused, for the person; a fault inside Elysium is no refusal and rolls
/// the whole change back.
fn refusal_message(error: WorkError) -> Result<String, WorkError> {
    match error {
        WorkError::Conflict(message) | WorkError::Invalid(message) => Ok(message.to_owned()),
        WorkError::Refused(message) => Ok(message),
        WorkError::Database(DieselError::NotFound) => Ok("no initiative has this id".to_owned()),
        fault @ WorkError::Database(_) => Err(fault),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;
    use crate::models::initiative::NewInitiative;
    use crate::test_support::{TEST_PERSON, migrated_database};

    #[test]
    fn changes_are_named_by_kind_and_refuse_unknown_fields() {
        let state: BulkInitiativeChange =
            serde_json::from_value(json!({ "kind": "state", "state": "achieved" }))
                .expect("parses");
        assert_eq!(
            state,
            BulkInitiativeChange::State {
                state: InitiativeState::Achieved
            }
        );
        serde_json::from_value::<BulkInitiativeChange>(json!({ "kind": "restore", "all": true }))
            .expect_err("unknown fields are refused");
        serde_json::from_value::<BulkInitiativeChange>(json!({ "kind": "rename" }))
            .expect_err("not a change");
    }

    fn minute(offset: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
            + TimeDelta::minutes(offset)
    }

    async fn create(connection: &mut AsyncPgConnection, name: &str) -> Uuid {
        let new_initiative = NewInitiative {
            name: name.to_owned(),
            description: String::new(),
            target_at: None,
            project_ids: Vec::new(),
        };
        initiative::create(connection, new_initiative, TEST_PERSON, minute(0))
            .await
            .expect("create")
            .record
            .id
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_selection_changes_state_together_and_a_deleted_one_refuses_alone() {
        let (_url, mut connection) = migrated_database().await;
        let launch = create(&mut connection, "Launch").await;
        let deploy = create(&mut connection, "Deploy").await;
        initiative::soft_delete(&mut connection, deploy, TEST_PERSON, minute(1))
            .await
            .expect("delete");
        let mut ids = vec![launch, deploy];
        ids.sort_unstable();

        let achieve = BulkInitiativeChange::State {
            state: InitiativeState::Achieved,
        };
        let outcome = change_many(&mut connection, &ids, achieve, TEST_PERSON, minute(2))
            .await
            .expect("change");
        assert_eq!(outcome.written.len(), 1);
        assert_eq!(outcome.written[0].record.state, InitiativeState::Achieved);
        assert_eq!(outcome.written[0].history.len(), 1);
        assert_eq!(outcome.refused.len(), 1);
        assert_eq!(outcome.refused[0].initiative_id, deploy);

        let again = change_many(&mut connection, &[launch], achieve, TEST_PERSON, minute(3))
            .await
            .expect("change");
        assert!(again.written[0].history.is_empty(), "already achieved");

        let restored = change_many(
            &mut connection,
            &ids,
            BulkInitiativeChange::Restore {},
            TEST_PERSON,
            minute(4),
        )
        .await
        .expect("restore");
        assert_eq!(restored.written.len(), 1, "only the deleted one restores");
        assert_eq!(restored.written[0].record.id, deploy);
        assert_eq!(restored.refused[0].initiative_id, launch);
    }
}
