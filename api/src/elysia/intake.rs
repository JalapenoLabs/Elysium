// Copyright © 2026 Jalapeno Labs

//! Turns changes announced on the event bus into Elysia's `event` turns.
//!
//! The intake listens to the same bus every browser does, keeps only the changes that can mean
//! work for Elysia ([`classify`]), and batches them: the first relevant change opens a batch,
//! and [`DEBOUNCE`] later the batch is queued as one turn. A burst (the watcher reading a
//! dozen issues, a changeset of many operations) therefore costs one turn, not a dozen. While
//! an `event` turn is still queued, a new batch joins it instead of queueing another.
//!
//! What she ignores: her own changes (actor `elysia`), what coding agents do (they report
//! through their own sessions), and anything a person did that is not new work or a note.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::{Level, event};
use uuid::Uuid;

use super::trigger;
use crate::database::Pool;
use crate::models::elysia::{self, NewTurn};
use crate::models::user::SYSTEM_USER_ID;
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::elysia::{TurnResponse, publish_snapshot};

/// How long a batch stays open after its first change. Long enough to gather a burst from the
/// watcher or a changeset being applied; short enough that she reacts while it is fresh.
pub const DEBOUNCE: Duration = Duration::from_secs(30);

/// The most changes one turn carries. A larger burst is counted rather than listed, so a
/// runaway source cannot build a prompt past what a turn can read.
pub const MAX_EVENTS_PER_TURN: usize = 50;

/// History kinds a person's own action counts for: new work, and notes about it.
const PERSON_KINDS: [&str; 2] = ["created", "commented"];

/// The change as the turn should see it, when `message` (one bus message) can mean work for
/// Elysia.
pub fn classify(message: &str) -> Option<Value> {
    let change: Value = serde_json::from_str(message).ok()?;
    let kind = change.get("type")?.as_str()?;
    let data = change.get("data")?;

    let relevant = match kind {
        "history.appended" => history_is_relevant(data),
        // She learns how a person decided on what she proposed.
        "changeset.upserted" => {
            data.get("proposer").and_then(Value::as_str) == Some("elysia")
                && data.get("state").and_then(Value::as_str) != Some("pending")
        }
        // Mail, chat, and meetings arrive here once connectors ingest them.
        "timelineRecord.created" => true,
        _ => false,
    };
    relevant.then_some(change)
}

fn history_is_relevant(data: &Value) -> bool {
    let actor = data
        .get("actor")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if actor == "elysia" || actor.starts_with("session:") {
        return false;
    }
    if actor.starts_with("watcher:") {
        return true;
    }
    let kind = data.get("kind").and_then(Value::as_str).unwrap_or_default();
    PERSON_KINDS.contains(&kind)
}

/// Adds `changes` to the trigger detail an `event` turn already carries, keeping at most
/// [`MAX_EVENTS_PER_TURN`] and counting the rest in `omitted`.
pub fn merge_events(detail: &Value, changes: Vec<Value>, omitted: u64) -> Value {
    let mut events: Vec<Value> = detail
        .get("events")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut omitted = omitted + detail.get("omitted").and_then(Value::as_u64).unwrap_or(0);
    for change in changes {
        if events.len() < MAX_EVENTS_PER_TURN {
            events.push(change);
        } else {
            omitted += 1;
        }
    }
    json!({ "events": events, "omitted": omitted })
}

/// What the intake needs.
#[derive(Clone)]
pub struct IntakeContext {
    pub database: Pool,
    pub events: EventBus,
}

/// Listens to the bus until `shutdown`, queueing batches as `event` turns.
pub async fn run(context: IntakeContext, shutdown: CancellationToken) {
    let mut receiver = context.events.subscribe();
    let mut batch: Vec<Value> = Vec::new();
    let mut missed: u64 = 0;
    let mut deadline: Option<Instant> = None;

    loop {
        let flush_at = deadline.unwrap_or_else(|| Instant::now() + DEBOUNCE);
        tokio::select! {
            () = shutdown.cancelled() => break,
            delivered = receiver.recv() => match delivered {
                Ok(message) => {
                    if let Some(change) = classify(&message) {
                        batch.push(change);
                        deadline.get_or_insert_with(|| Instant::now() + DEBOUNCE);
                    }
                }
                Err(RecvError::Lagged(skipped)) => {
                    // The bus outran the intake. What was skipped is counted, not guessed at.
                    event!(
                        name: "elysia.intake.lagged",
                        Level::WARN,
                        elysia.intake.skipped = skipped,
                        "the intake fell behind the event bus and skipped {{elysia.intake.skipped}} events",
                    );
                    missed += skipped;
                    deadline.get_or_insert_with(|| Instant::now() + DEBOUNCE);
                }
                Err(RecvError::Closed) => break,
            },
            () = tokio::time::sleep_until(flush_at), if deadline.is_some() => {
                let changes = std::mem::take(&mut batch);
                let skipped = std::mem::take(&mut missed);
                deadline = None;
                if let Err(error) = queue(&context, changes, skipped).await {
                    event!(
                        name: "elysia.intake.queue.failure",
                        Level::ERROR,
                        error.message = %error,
                        "could not queue an event turn; the batch is lost",
                    );
                }
            }
        }
    }
    event!(name: "elysia.intake.stopped", Level::INFO, "the intake stopped");
}

/// Queues `changes` as an `event` turn, joining one still queued.
async fn queue(context: &IntakeContext, changes: Vec<Value>, missed: u64) -> anyhow::Result<()> {
    let mut connection = context.database.get().await?;
    // Join the queued turn if there is one. It may start between the read and the write, in
    // which case the batch gets a turn of its own instead.
    let joined =
        if let Some(queued) = elysia::queued_turn_of_kind(&mut connection, trigger::EVENT).await? {
            let detail = merge_events(&queued.trigger_detail, changes.clone(), missed);
            elysia::set_trigger_detail(&mut connection, queued.id, detail).await?
        } else {
            None
        };
    let turn = if let Some(turn) = joined {
        turn
    } else {
        elysia::insert_turn(
            &mut connection,
            NewTurn {
                id: Uuid::now_v7(),
                trigger_kind: trigger::EVENT.to_owned(),
                trigger_detail: merge_events(&json!({}), changes, missed),
                created_by: SYSTEM_USER_ID,
            },
        )
        .await?
    };

    context
        .events
        .publish(&ServerEvent::ElysiaTurnUpserted(TurnResponse::from(turn)));
    publish_snapshot(&context.events, &mut connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::elysia::TurnStatus;

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run scripts/verify-migrations.sh"]
    async fn batches_join_the_event_turn_still_queued() {
        let (url, mut connection) = crate::test_support::migrated_database().await;
        let context = IntakeContext {
            database: crate::connections::connect_postgres(&url, 2)
                .await
                .expect("Postgres accepts connections"),
            events: EventBus::new(),
        };

        queue(&context, vec![json!({ "type": "first" })], 0)
            .await
            .expect("the first batch is queued");
        queue(&context, vec![json!({ "type": "second" })], 2)
            .await
            .expect("the second batch is queued");

        let active = elysia::active_turns(&mut connection)
            .await
            .expect("turns load");
        assert_eq!(active.len(), 1, "the second batch joined the first turn");
        assert_eq!(active[0].status, TurnStatus::Queued);
        assert_eq!(active[0].trigger_kind, trigger::EVENT);
        assert_eq!(
            active[0].trigger_detail,
            json!({ "events": [{ "type": "first" }, { "type": "second" }], "omitted": 2 })
        );
    }

    fn bus(kind: &str, data: &Value) -> String {
        json!({ "type": kind, "data": data }).to_string()
    }

    #[test]
    fn provider_changes_and_new_work_are_relevant() {
        let watcher = bus(
            "history.appended",
            &json!({ "actor": "watcher:jira", "kind": "state_changed" }),
        );
        assert!(classify(&watcher).is_some());

        let created = bus(
            "history.appended",
            &json!({ "actor": "user:0199a3c4-0000-7000-8000-00000000beef", "kind": "created" }),
        );
        assert!(classify(&created).is_some());

        let record = bus("timelineRecord.created", &json!({ "id": "x" }));
        assert!(classify(&record).is_some());
    }

    #[test]
    fn her_own_changes_and_coding_agents_are_ignored() {
        for actor in ["elysia", "session:12"] {
            let change = bus(
                "history.appended",
                &json!({ "actor": actor, "kind": "created" }),
            );
            assert!(classify(&change).is_none(), "{actor}");
        }
    }

    #[test]
    fn a_person_snoozing_or_editing_is_not_work_for_her() {
        let updated = bus(
            "history.appended",
            &json!({ "actor": "user:0199a3c4-0000-7000-8000-00000000beef", "kind": "updated" }),
        );
        assert!(classify(&updated).is_none());
    }

    #[test]
    fn only_decisions_on_her_own_changesets_count() {
        let applied = bus(
            "changeset.upserted",
            &json!({ "proposer": "elysia", "state": "applied" }),
        );
        assert!(classify(&applied).is_some());

        let pending = bus(
            "changeset.upserted",
            &json!({ "proposer": "elysia", "state": "pending" }),
        );
        assert!(
            classify(&pending).is_none(),
            "her own proposal is not news to her"
        );

        let agent = bus(
            "changeset.upserted",
            &json!({ "proposer": "session:4", "state": "applied" }),
        );
        assert!(classify(&agent).is_none());
    }

    #[test]
    fn unrelated_and_malformed_messages_are_ignored() {
        assert!(classify(&bus("llm.upserted", &json!({}))).is_none());
        assert!(classify("not json").is_none());
        assert!(classify(r#"{"type":"hello"}"#).is_none());
    }

    #[test]
    fn a_batch_joins_the_queued_turn_up_to_the_cap() {
        let first = merge_events(&json!({}), vec![json!(1), json!(2)], 0);
        assert_eq!(first, json!({ "events": [1, 2], "omitted": 0 }));

        let many: Vec<Value> = (0..MAX_EVENTS_PER_TURN).map(|index| json!(index)).collect();
        let merged = merge_events(&first, many, 3);
        assert_eq!(
            merged["events"].as_array().map(Vec::len),
            Some(MAX_EVENTS_PER_TURN)
        );
        assert_eq!(merged["omitted"], json!(5), "2 past the cap plus 3 missed");
    }
}
