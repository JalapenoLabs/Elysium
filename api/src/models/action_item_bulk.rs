// Copyright © 2026 Jalapeno Labs

//! One change applied to many action items at once, as a person's multi-select asks.
//!
//! Every item goes through the same write its single-item route uses
//! ([`action_item::transition`], [`action_item::update`], and the rest), so each is
//! validated, locked, and recorded in its own history exactly as it would be alone. The
//! whole change runs in one transaction. Each item writes in a savepoint inside it: an item
//! the records refuse (deleted, or in a state the change cannot leave) rolls back alone and
//! is reported with the reason, and the rest apply. A fault inside Elysium rolls every item
//! back. An item that already holds what the change asks for records nothing.
//!
//! Items are written in the order of their ids, so two changes over overlapping items lock
//! rows in the same order and never deadlock.

use chrono::{DateTime, Utc};
use diesel::result::Error as DieselError;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::action_items::{Actor, Transition, WorkError};
use crate::models::action_item::{
    self, ActionItem, ActionItemChanges, ActionItemPriority, ActionItemState,
};
use crate::models::action_item_event::Recorded;
use crate::models::initiative;

/// The most items one change names.
///
/// Every item is written in one transaction, so the limit keeps that transaction, and the
/// burst of events it publishes afterwards, short. It is far above a page of the item list,
/// and selecting every item across pages of a larger list asks for the change in parts.
pub const MAX_ITEMS: usize = 1000;

/// What to do to every item named. Each is a write a single-item route also makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BulkChange {
    /// Moves each item to `state` by the transition that leads there from its own state
    /// ([`Transition::toward`]): accept or reopen for `open`, resolve, or dismiss.
    State {
        state: ActionItemState,
    },
    Priority {
        priority: ActionItemPriority,
    },
    /// Hides each item from Next until `until`, a moment in the future, or wakes it for
    /// `null`. Required either way: naming the deserializer keeps serde from reading an
    /// absent `until` as `null`.
    Snooze {
        #[serde(deserialize_with = "Option::deserialize")]
        until: Option<DateTime<Utc>>,
    },
    JoinInitiative {
        initiative_id: Uuid,
    },
    LeaveInitiative {
        initiative_id: Uuid,
    },
    /// Deletes softly, so every item can be restored.
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

/// An item the change did not apply to, and why, in words for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub item_id: Uuid,
    pub message: String,
}

/// What a change did.
#[derive(Debug, Default)]
pub struct BulkOutcome {
    /// Every item the change applied to, as it stands now, with the history it recorded:
    /// none for an item that already held what was asked.
    pub written: Vec<Recorded<ActionItem>>,
    pub refused: Vec<Refusal>,
}

/// Applies `change` to every item in `item_ids`, which must be sorted and free of repeats.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for joining an initiative that does not
/// exist, [`WorkError::Conflict`] for joining a deleted one, and any database fault, which
/// writes nothing. An item the records refuse is reported in [`BulkOutcome::refused`].
pub async fn change_many(
    connection: &mut AsyncPgConnection,
    item_ids: &[Uuid],
    change: BulkChange,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<BulkOutcome, WorkError> {
    connection
        .transaction(async move |connection| {
            // An initiative that cannot be joined refuses the whole change, not each item.
            if let BulkChange::JoinInitiative { initiative_id } = change {
                let joining = initiative::find(connection, initiative_id).await?;
                if joining.deleted_at.is_some() {
                    return Err(WorkError::Conflict("the initiative is deleted"));
                }
            }

            let mut outcome = BulkOutcome::default();
            for &item_id in item_ids {
                let attempt = connection
                    .transaction(async move |connection| {
                        change_one(connection, item_id, change, actor, now).await
                    })
                    .await;
                match attempt {
                    Ok(written) => outcome.written.push(written),
                    Err(error) => outcome.refused.push(Refusal {
                        item_id,
                        message: refusal_message(error)?,
                    }),
                }
            }
            Ok(outcome)
        })
        .await
}

/// Applies the change to one item through the write its single-item route makes.
async fn change_one(
    connection: &mut AsyncPgConnection,
    item_id: Uuid,
    change: BulkChange,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    match change {
        BulkChange::State { state } => {
            let item = action_item::lock_live(connection, item_id).await?;
            match Transition::toward(item.state, state).map_err(WorkError::Conflict)? {
                Some(transition) => {
                    action_item::transition(connection, item_id, transition, actor, now).await
                }
                None => Ok(Recorded {
                    record: item,
                    history: Vec::new(),
                }),
            }
        }
        BulkChange::Priority { priority } => {
            let changes = ActionItemChanges {
                priority: Some(priority),
                ..ActionItemChanges::default()
            };
            action_item::update(connection, item_id, changes, actor, now).await
        }
        BulkChange::Snooze { until } => {
            let changes = ActionItemChanges {
                snoozed_until: Some(until),
                ..ActionItemChanges::default()
            };
            action_item::update(connection, item_id, changes, actor, now).await
        }
        BulkChange::JoinInitiative { initiative_id } => {
            action_item::join_initiative(connection, item_id, initiative_id, actor, now).await
        }
        BulkChange::LeaveInitiative { initiative_id } => {
            action_item::leave_initiative(connection, item_id, initiative_id, actor, now).await
        }
        BulkChange::Delete {} => action_item::soft_delete(connection, item_id, actor, now).await,
        BulkChange::Restore {} => action_item::restore(connection, item_id, actor, now).await,
    }
}

/// Why an item refused, for the person; a fault inside Elysium is no refusal and rolls the
/// whole change back.
fn refusal_message(error: WorkError) -> Result<String, WorkError> {
    match error {
        WorkError::Conflict(message) | WorkError::Invalid(message) => Ok(message.to_owned()),
        WorkError::Refused(message) => Ok(message),
        WorkError::Database(DieselError::NotFound) => Ok("no action item has this id".to_owned()),
        fault @ WorkError::Database(_) => Err(fault),
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;
    use crate::models::action_item::{NewActionItem, Owner};
    use crate::models::action_item_event::list_for_item;
    use crate::models::initiative::NewInitiative;
    use crate::test_support::{TEST_PERSON, migrated_database};

    #[test]
    fn changes_are_named_by_kind_with_their_fields() {
        let state: BulkChange =
            serde_json::from_value(json!({ "kind": "state", "state": "resolved" }))
                .expect("parses");
        assert_eq!(
            state,
            BulkChange::State {
                state: ActionItemState::Resolved
            }
        );

        let initiative_id = Uuid::now_v7();
        let join: BulkChange = serde_json::from_value(
            json!({ "kind": "join-initiative", "initiativeId": initiative_id }),
        )
        .expect("parses");
        assert_eq!(join, BulkChange::JoinInitiative { initiative_id });

        let delete: BulkChange =
            serde_json::from_value(json!({ "kind": "delete" })).expect("parses");
        assert_eq!(delete, BulkChange::Delete {});
    }

    #[test]
    fn a_snooze_names_its_end_or_null_and_nothing_unknown_passes() {
        let wake: BulkChange =
            serde_json::from_value(json!({ "kind": "snooze", "until": null })).expect("parses");
        assert_eq!(wake, BulkChange::Snooze { until: None });

        serde_json::from_value::<BulkChange>(json!({ "kind": "snooze" }))
            .expect_err("until is required");
        serde_json::from_value::<BulkChange>(json!({ "kind": "snooze", "until": "2026-10-01" }))
            .expect_err("no offset");
        serde_json::from_value::<BulkChange>(json!({ "kind": "rename", "title": "x" }))
            .expect_err("not a change");
        serde_json::from_value::<BulkChange>(json!({ "kind": "delete", "force": true }))
            .expect_err("unknown fields are refused");
    }

    fn minute(offset: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
            + TimeDelta::minutes(offset)
    }

    async fn create(
        connection: &mut AsyncPgConnection,
        title: &str,
        state: ActionItemState,
    ) -> Uuid {
        let item = NewActionItem {
            title: title.to_owned(),
            notes: String::new(),
            state,
            priority: ActionItemPriority::Normal,
            due_at: None,
            owner: Owner::User,
            project_ids: Vec::new(),
            initiative_ids: Vec::new(),
        };
        action_item::create(connection, item, TEST_PERSON, minute(0))
            .await
            .expect("create")
            .record
            .id
    }

    async fn history_kinds(connection: &mut AsyncPgConnection, id: Uuid) -> Vec<String> {
        list_for_item(connection, id)
            .await
            .expect("history")
            .into_iter()
            .map(|entry| entry.kind)
            .collect()
    }

    fn sorted(mut ids: Vec<Uuid>) -> Vec<Uuid> {
        ids.sort_unstable();
        ids
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_state_change_moves_each_item_its_own_way_and_reports_what_refused() {
        let (_url, mut connection) = migrated_database().await;
        let inbox = create(&mut connection, "inbox", ActionItemState::Inbox).await;
        let open = create(&mut connection, "open", ActionItemState::Open).await;
        let resolved = create(&mut connection, "resolved", ActionItemState::Open).await;
        action_item::transition(
            &mut connection,
            resolved,
            Transition::Resolve,
            TEST_PERSON,
            minute(1),
        )
        .await
        .expect("resolve");
        let unknown = Uuid::now_v7();

        let change = BulkChange::State {
            state: ActionItemState::Dismissed,
        };
        let ids = sorted(vec![inbox, open, resolved, unknown]);
        let outcome = change_many(&mut connection, &ids, change, TEST_PERSON, minute(2))
            .await
            .expect("change");

        let dismissed: Vec<Uuid> = outcome
            .written
            .iter()
            .map(|written| written.record.id)
            .collect();
        assert_eq!(dismissed, sorted(vec![inbox, open]));
        assert!(
            outcome
                .written
                .iter()
                .all(|written| written.record.state == ActionItemState::Dismissed)
        );
        assert_eq!(
            outcome.refused,
            sorted(vec![resolved, unknown])
                .into_iter()
                .map(|item_id| Refusal {
                    item_id,
                    message: if item_id == resolved {
                        "only an item in the inbox or open can be dismissed".to_owned()
                    } else {
                        "no action item has this id".to_owned()
                    },
                })
                .collect::<Vec<_>>()
        );
        assert_eq!(
            history_kinds(&mut connection, open).await,
            ["created", "state_changed"],
            "each item records its own change"
        );
        assert_eq!(
            history_kinds(&mut connection, resolved).await,
            ["created", "state_changed"],
            "the refused item kept only its own resolve"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_item_already_there_records_nothing_and_deleted_items_restore_together() {
        let (_url, mut connection) = migrated_database().await;
        let first = create(&mut connection, "first", ActionItemState::Open).await;
        let second = create(&mut connection, "second", ActionItemState::Open).await;
        let ids = sorted(vec![first, second]);

        let reopen = BulkChange::State {
            state: ActionItemState::Open,
        };
        let unchanged = change_many(&mut connection, &ids, reopen, TEST_PERSON, minute(1))
            .await
            .expect("change");
        assert!(unchanged.refused.is_empty());
        assert!(
            unchanged
                .written
                .iter()
                .all(|written| written.history.is_empty())
        );

        let deleted = change_many(
            &mut connection,
            &ids,
            BulkChange::Delete {},
            TEST_PERSON,
            minute(2),
        )
        .await
        .expect("delete");
        assert!(
            deleted
                .written
                .iter()
                .all(|written| written.record.deleted_at == Some(minute(2)))
        );

        let refused = change_many(
            &mut connection,
            &ids,
            BulkChange::Priority {
                priority: ActionItemPriority::Urgent,
            },
            TEST_PERSON,
            minute(3),
        )
        .await
        .expect("change");
        assert_eq!(refused.refused.len(), 2, "a deleted item refuses edits");

        let restored = change_many(
            &mut connection,
            &ids,
            BulkChange::Restore {},
            TEST_PERSON,
            minute(4),
        )
        .await
        .expect("restore");
        assert!(
            restored
                .written
                .iter()
                .all(|written| written.record.deleted_at.is_none())
        );
        assert_eq!(
            history_kinds(&mut connection, first).await,
            ["created", "deleted", "restored"]
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn joining_a_deleted_initiative_refuses_the_whole_change() {
        let (_url, mut connection) = migrated_database().await;
        let item = create(&mut connection, "item", ActionItemState::Open).await;
        let launch = initiative::create(
            &mut connection,
            NewInitiative {
                name: "Launch".to_owned(),
                description: String::new(),
                target_at: None,
                project_ids: Vec::new(),
            },
            TEST_PERSON,
            minute(0),
        )
        .await
        .expect("initiative")
        .record;

        let join = BulkChange::JoinInitiative {
            initiative_id: launch.id,
        };
        let joined = change_many(&mut connection, &[item], join, TEST_PERSON, minute(1))
            .await
            .expect("join");
        assert_eq!(joined.written[0].history.len(), 1);

        initiative::soft_delete(&mut connection, launch.id, TEST_PERSON, minute(2))
            .await
            .expect("delete");
        let refused = change_many(&mut connection, &[item], join, TEST_PERSON, minute(3)).await;
        assert!(matches!(refused, Err(WorkError::Conflict(_))));

        let unknown = BulkChange::JoinInitiative {
            initiative_id: Uuid::now_v7(),
        };
        let missing = change_many(&mut connection, &[item], unknown, TEST_PERSON, minute(4)).await;
        assert!(matches!(
            missing,
            Err(WorkError::Database(DieselError::NotFound))
        ));
    }
}
