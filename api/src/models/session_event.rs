// Copyright © 2026 Jalapeno Labs

//! Every event a session's turns reported, kept for as long as the session.
//!
//! Events are stored as the runner reported them (`crate::runner::RunnerEvent`), as JSON,
//! rather than as the view clients see: a later build renders old events with whatever it knows
//! then. Sequences are Elysium's own, strictly increasing per session (`crate::sessions`).

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::on_constraint;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::session_events;
use crate::runner::RunnerEvent;

/// One recorded event.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub sequence: u64,
    pub turn_id: Option<Uuid>,
    pub occurred_at: DateTime<Utc>,
    pub event: RunnerEvent,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = session_events)]
struct NewSessionEvent {
    session_id: i64,
    sequence: i64,
    occurred_at: DateTime<Utc>,
    turn_id: Option<Uuid>,
    payload: Value,
}

/// Records `stored`. An event already recorded under its sequence is left as it is.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for a session that was
/// deleted meanwhile.
pub async fn record(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    stored: &StoredEvent,
) -> QueryResult<()> {
    let payload = serde_json::to_value(&stored.event)
        .map_err(|error| diesel::result::Error::SerializationError(Box::new(error)))?;
    let row = NewSessionEvent {
        session_id,
        sequence: sequence_to_column(stored.sequence),
        occurred_at: stored.occurred_at,
        turn_id: stored.turn_id,
        payload,
    };
    diesel::insert_into(session_events::table)
        .values(row)
        .on_conflict(on_constraint("session_events_pkey"))
        .do_nothing()
        .execute(connection)
        .await?;
    Ok(())
}

/// The last sequence recorded for a session; `None` before its first event.
///
/// # Errors
/// Propagates any database error.
pub async fn latest_sequence(
    connection: &mut AsyncPgConnection,
    session_id: i64,
) -> QueryResult<Option<u64>> {
    let latest: Option<i64> = session_events::table
        .filter(session_events::session_id.eq(session_id))
        .select(diesel::dsl::max(session_events::sequence))
        .first(connection)
        .await?;
    Ok(latest.map(sequence_from_column))
}

type Row = (i64, Option<Uuid>, DateTime<Utc>, Value);

/// A session's most recent events, oldest first, at most `limit` of them, and whether older
/// ones were left out.
///
/// # Errors
/// Propagates any database error.
pub async fn latest(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    limit: usize,
) -> QueryResult<(Vec<StoredEvent>, bool)> {
    let fetch = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
    let mut rows: Vec<Row> = session_events::table
        .filter(session_events::session_id.eq(session_id))
        .order(session_events::sequence.desc())
        .limit(fetch)
        .select(columns())
        .load(connection)
        .await?;

    let truncated = rows.len() > limit;
    rows.truncate(limit);
    rows.reverse();
    Ok((decode_rows(session_id, rows), truncated))
}

/// A session's events after `after_sequence`, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn after(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    after_sequence: u64,
) -> QueryResult<Vec<StoredEvent>> {
    let rows: Vec<Row> = session_events::table
        .filter(session_events::session_id.eq(session_id))
        .filter(session_events::sequence.gt(sequence_to_column(after_sequence)))
        .order(session_events::sequence.asc())
        .select(columns())
        .load(connection)
        .await?;
    Ok(decode_rows(session_id, rows))
}

/// What the turns of coding and Studio sessions spent, summed from each turn's `finished`
/// event: the cost and tokens Claude Code reported, over every attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, QueryableByName, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedUsage {
    /// Turns that finished, whether or not they reported a cost.
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub turns: i64,
    /// Millionths of a US dollar. On a subscription it is what the same tokens would cost on
    /// the API, since Claude Code reports that either way.
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub cost_usd_micros: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub input_tokens: i64,
    #[diesel(sql_type = diesel::sql_types::BigInt)]
    pub output_tokens: i64,
}

/// What session turns that finished at or after `since` spent, every session's, or with
/// `project` only those of that project's sessions.
///
/// # Errors
/// Propagates any database error.
pub async fn usage_since(
    connection: &mut AsyncPgConnection,
    since: DateTime<Utc>,
    project: Option<Uuid>,
) -> QueryResult<RecordedUsage> {
    // `session_events_finished_at` holds exactly these rows by time, so a window reads only
    // the turns' last events. `SUM(BIGINT)` is `NUMERIC`, so each sum is cast back in SQL.
    diesel::sql_query(
        "SELECT \
            COUNT(*) AS turns, \
            COALESCE(SUM((event.payload ->> 'costUsdMicros')::BIGINT), 0)::BIGINT \
                AS cost_usd_micros, \
            COALESCE(SUM((event.payload ->> 'inputTokens')::BIGINT), 0)::BIGINT AS input_tokens, \
            COALESCE(SUM((event.payload ->> 'outputTokens')::BIGINT), 0)::BIGINT \
                AS output_tokens \
        FROM session_events event \
        JOIN coding_sessions session ON session.id = event.session_id \
        WHERE event.payload ->> 'kind' = 'finished' \
            AND event.occurred_at >= $1 \
            AND ($2::UUID IS NULL OR session.project_id = $2)",
    )
    .bind::<diesel::sql_types::Timestamptz, _>(since)
    .bind::<diesel::sql_types::Nullable<diesel::sql_types::Uuid>, _>(project)
    .get_result(connection)
    .await
}

const fn columns() -> (
    session_events::sequence,
    session_events::turn_id,
    session_events::occurred_at,
    session_events::payload,
) {
    (
        session_events::sequence,
        session_events::turn_id,
        session_events::occurred_at,
        session_events::payload,
    )
}

/// Decodes stored events in order. An event this build no longer reads is skipped and logged:
/// one bad row must not hide the rest of a conversation.
fn decode_rows(session_id: i64, rows: Vec<Row>) -> Vec<StoredEvent> {
    let mut events = Vec::with_capacity(rows.len());
    for (sequence, turn_id, occurred_at, payload) in rows {
        match serde_json::from_value::<RunnerEvent>(payload) {
            Ok(event) => events.push(StoredEvent {
                sequence: sequence_from_column(sequence),
                turn_id,
                occurred_at,
                event,
            }),
            Err(error) => event!(
                name: "session_event.decode.failure",
                Level::ERROR,
                session.id = session_id,
                event.sequence = sequence,
                error.message = %error,
                "a stored event no longer decodes; it was left out of the history",
            ),
        }
    }
    events
}

/// Sequences start at 1 and grow by one per event, so they never reach `i64::MAX`.
fn sequence_to_column(sequence: u64) -> i64 {
    i64::try_from(sequence).unwrap_or(i64::MAX)
}

/// The column is checked non-negative, so the conversion back never loses anything.
fn sequence_from_column(sequence: i64) -> u64 {
    u64::try_from(sequence).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::coding_session::{self, NewCodingSession};
    use crate::models::project::{self, NewProject};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    fn message(sequence: u64, text: &str) -> StoredEvent {
        StoredEvent {
            sequence,
            turn_id: Some(Uuid::now_v7()),
            occurred_at: Utc::now(),
            event: RunnerEvent::AgentMessage {
                text: text.to_owned(),
            },
        }
    }

    async fn session(connection: &mut AsyncPgConnection) -> i64 {
        let project_id = project::create(
            connection,
            &NewProject {
                created_by: TEST_PERSON_ID,
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project")
        .id;
        let new_session = NewCodingSession {
            created_by: TEST_PERSON_ID,
            id: coding_session::reserve_id(connection)
                .await
                .expect("reserve"),
            project_id: Some(project_id),
            title: "Session".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
        };
        coding_session::create(connection, &new_session)
            .await
            .expect("session")
            .id
    }

    #[test]
    fn sequences_round_trip_through_the_column() {
        assert_eq!(sequence_from_column(sequence_to_column(42)), 42);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn events_are_kept_once_and_read_back_in_order() {
        let (_url, mut connection) = migrated_database().await;
        let session_id = session(&mut connection).await;
        assert_eq!(
            latest_sequence(&mut connection, session_id)
                .await
                .expect("latest"),
            None
        );

        for sequence in 1..=5 {
            record(
                &mut connection,
                session_id,
                &message(sequence, &format!("m{sequence}")),
            )
            .await
            .expect("record");
        }
        record(&mut connection, session_id, &message(3, "again"))
            .await
            .expect("a duplicate is ignored");
        assert_eq!(
            latest_sequence(&mut connection, session_id)
                .await
                .expect("latest"),
            Some(5)
        );

        let (all, truncated) = latest(&mut connection, session_id, 10).await.expect("read");
        assert!(!truncated);
        let sequences: Vec<u64> = all.iter().map(|event| event.sequence).collect();
        assert_eq!(sequences, [1, 2, 3, 4, 5]);
        assert!(matches!(&all[2].event, RunnerEvent::AgentMessage { text } if text == "m3"));

        let (recent, truncated) = latest(&mut connection, session_id, 2).await.expect("read");
        assert!(truncated);
        let sequences: Vec<u64> = recent.iter().map(|event| event.sequence).collect();
        assert_eq!(sequences, [4, 5]);

        let rest = after(&mut connection, session_id, 3).await.expect("read");
        let sequences: Vec<u64> = rest.iter().map(|event| event.sequence).collect();
        assert_eq!(sequences, [4, 5]);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_sessions_events_go_with_it() {
        let (_url, mut connection) = migrated_database().await;
        let session_id = session(&mut connection).await;
        record(&mut connection, session_id, &message(1, "hello"))
            .await
            .expect("record");
        coding_session::delete(&mut connection, session_id)
            .await
            .expect("delete");
        let (events, _truncated) = latest(&mut connection, session_id, 10).await.expect("read");
        assert!(events.is_empty());
    }
}
