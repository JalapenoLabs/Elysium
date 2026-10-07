// Copyright © 2026 Jalapeno Labs

//! Every event a session's thread emitted, kept after the thread is gone.
//!
//! Satellites keep a thread's events only while the thread lives, so Elysium records each
//! one as its session watcher receives it (`crate::fleet`), and a session's history is read
//! from here. Events are stored as the satellite encoded them, a protobuf `ThreadEvent`,
//! rather than as Elysium's JSON view: a later build renders old events with whatever it
//! knows then, including payloads this one leaves unrendered.

use arsox_sdk::proto::event::v1::ThreadEvent;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::on_constraint;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use prost::Message as _;
use tracing::{Level, event};

use crate::database::schema::session_events;

#[derive(Debug, Insertable)]
#[diesel(table_name = session_events)]
struct NewSessionEvent {
    session_id: i64,
    sequence: i64,
    occurred_at: Option<DateTime<Utc>>,
    event: Vec<u8>,
}

/// Records one event. An event already recorded, as a replay after a reconnect delivers
/// again, is left as it is.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for a session that was
/// deleted meanwhile.
pub async fn record(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    thread_event: &ThreadEvent,
) -> QueryResult<()> {
    let row = NewSessionEvent {
        session_id,
        sequence: sequence_to_column(thread_event.sequence),
        occurred_at: thread_event
            .occurred_at
            .as_ref()
            .and_then(|timestamp| DateTime::from_timestamp(timestamp.epoch_seconds, timestamp.nanos)),
        event: thread_event.encode_to_vec(),
    };
    diesel::insert_into(session_events::table)
        .values(row)
        .on_conflict(on_constraint("session_events_pkey"))
        .do_nothing()
        .execute(connection)
        .await?;
    Ok(())
}

/// The last sequence recorded for a session, where its watcher resumes; `None` before its
/// first event.
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

/// A session's most recent events, oldest first, at most `limit` of them, and whether older
/// ones were left out.
///
/// An event whose stored bytes no longer decode is skipped and logged: one bad row must not
/// hide the rest of a conversation.
///
/// # Errors
/// Propagates any database error.
pub async fn latest(
    connection: &mut AsyncPgConnection,
    session_id: i64,
    limit: usize,
) -> QueryResult<(Vec<ThreadEvent>, bool)> {
    let fetch = i64::try_from(limit.saturating_add(1)).unwrap_or(i64::MAX);
    let mut rows: Vec<(i64, Vec<u8>)> = session_events::table
        .filter(session_events::session_id.eq(session_id))
        .order(session_events::sequence.desc())
        .limit(fetch)
        .select((session_events::sequence, session_events::event))
        .load(connection)
        .await?;

    let truncated = rows.len() > limit;
    rows.truncate(limit);
    rows.reverse();

    let mut events = Vec::with_capacity(rows.len());
    for (sequence, bytes) in rows {
        match ThreadEvent::decode(bytes.as_slice()) {
            Ok(thread_event) => events.push(thread_event),
            Err(error) => event!(
                name: "session_event.decode.failure",
                Level::ERROR,
                session.id = session_id,
                event.sequence = sequence,
                error.message = %error,
                "a stored thread event no longer decodes; it was left out of the history",
            ),
        }
    }
    Ok((events, truncated))
}

/// Sequences are `u64` on the wire and `BIGINT` in Postgres. A satellite would have to emit
/// 2^63 events on one thread to overflow, so the conversion saturates rather than fails.
fn sequence_to_column(sequence: u64) -> i64 {
    i64::try_from(sequence).unwrap_or(i64::MAX)
}

/// The column is checked non-negative, so the conversion back never loses anything.
fn sequence_from_column(sequence: i64) -> u64 {
    u64::try_from(sequence).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use arsox_sdk::proto::event::v1::{AgentMessage, thread_event};
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::*;
    use crate::models::coding_session::{self, NewCodingSession};
    use crate::models::project::{self, NewProject};
    use crate::models::satellite::{self, NewSatellite};
    use crate::test_support::{cipher, migrated_database};

    fn message(sequence: u64, text: &str) -> ThreadEvent {
        ThreadEvent {
            sequence,
            r#type: "agent.message".to_owned(),
            payload: Some(thread_event::Payload::AgentMessage(AgentMessage {
                text: text.to_owned(),
                ..AgentMessage::default()
            })),
            ..ThreadEvent::default()
        }
    }

    async fn session(connection: &mut AsyncPgConnection) -> i64 {
        let satellite_id = satellite::create(
            connection,
            &cipher(),
            &NewSatellite {
                created_by: crate::test_support::TEST_PERSON_ID,
                name: "orbit".to_owned(),
                description: String::new(),
                url: "http://arsox:8080".to_owned(),
                secret: SecretString::from("bearer"),
                is_active: true,
            },
        )
        .await
        .expect("satellite")
        .id;
        let project_id = project::create(
            connection,
            &NewProject {
                created_by: crate::test_support::TEST_PERSON_ID,
                name: "Elysium".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project")
        .id;
        let new_session = NewCodingSession {
            created_by: crate::test_support::TEST_PERSON_ID,
            id: coding_session::reserve_id(connection).await.expect("reserve"),
            project_id: Some(project_id),
            satellite_id,
            thread_id: Uuid::now_v7().to_string(),
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
        assert_eq!(sequence_to_column(u64::MAX), i64::MAX);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn events_are_kept_once_and_read_back_newest_last() {
        let (_url, mut connection) = migrated_database().await;
        let session_id = session(&mut connection).await;
        assert_eq!(
            latest_sequence(&mut connection, session_id).await.expect("latest"),
            None
        );

        for sequence in 1..=5 {
            record(&mut connection, session_id, &message(sequence, &format!("m{sequence}")))
                .await
                .expect("record");
        }
        // A replay after a reconnect delivers an event again; the first copy stands.
        record(&mut connection, session_id, &message(3, "replayed"))
            .await
            .expect("a duplicate is ignored");

        assert_eq!(
            latest_sequence(&mut connection, session_id).await.expect("latest"),
            Some(5)
        );

        let (all, truncated) = latest(&mut connection, session_id, 10).await.expect("read");
        assert!(!truncated);
        let sequences: Vec<u64> = all.iter().map(|event| event.sequence).collect();
        assert_eq!(sequences, [1, 2, 3, 4, 5]);
        let Some(thread_event::Payload::AgentMessage(third)) = &all[2].payload else {
            panic!("the payload decodes: {:?}", all[2]);
        };
        assert_eq!(third.text, "m3");

        let (recent, truncated) = latest(&mut connection, session_id, 2).await.expect("read");
        assert!(truncated);
        let sequences: Vec<u64> = recent.iter().map(|event| event.sequence).collect();
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
