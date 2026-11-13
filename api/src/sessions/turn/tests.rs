// Copyright © 2026 Jalapeno Labs

//! A turn's live events as every client hears them (#278): whole while it has read no meeting,
//! then by their ids alone, since what it says can quote a meeting only its readers read. Every
//! event is recorded either way, for the conversation's history to answer each reader what they
//! may.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use super::Follower;
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::meeting_reads::{self, Turn};
use crate::models::project::{self, NewProject};
use crate::models::session_event;
use crate::models::session_turn::{self, NewSessionTurn};
use crate::runner::RunnerEvent;
use crate::sessions::Sessions;
use crate::sessions::scrub::Scrubber;
use crate::test_support::{TEST_PERSON_ID, migrated_database};
use crate::tools::grants::Grants;
use crate::tools::{CallScope, SessionScope};

/// A session's turn, started, as the queue starts one: its project, its session, and its id.
async fn started_turn(connection: &mut diesel_async::AsyncPgConnection) -> (Uuid, i64, Uuid) {
    let project_id = project::create(
        connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Reorg".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("a project")
    .id;
    let session_id = coding_session::reserve_id(connection)
        .await
        .expect("a number");
    coding_session::create(
        connection,
        &NewCodingSession {
            id: session_id,
            project_id: Some(project_id),
            title: "Reorg notes".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("a session");
    let turn = session_turn::queue(
        connection,
        &NewSessionTurn {
            id: Uuid::now_v7(),
            session_id,
            prompt: "What did the one-to-one decide?",
            system_prompt_append: None,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("queued");
    session_turn::take_next(connection).await.expect("taken");
    session_turn::record_started(connection, turn.id)
        .await
        .expect("started");
    (project_id, session_id, turn.id)
}

/// The next event about a session's events every client hears, skipping every other kind.
async fn next_heard(heard: &mut broadcast::Receiver<std::sync::Arc<str>>) -> (Value, String) {
    loop {
        let payload = tokio::time::timeout(Duration::from_secs(10), heard.recv())
            .await
            .expect("an event in time")
            .expect("the bus is open");
        let event: Value = serde_json::from_str(&payload).expect("JSON");
        if event["type"]
            .as_str()
            .is_some_and(|kind| kind.starts_with("session.event"))
        {
            return (event, payload.to_string());
        }
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn a_turn_that_read_a_meeting_announces_its_events_by_their_ids_alone() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("Postgres accepts connections");
    let grants = Grants::new(database.clone());
    let queue = crate::sessions::restart_tests::queue(&database, grants.clone());
    let mut heard = queue.inner.sessions.inner.events.subscribe();
    let (project_id, session_id, turn_id) = started_turn(&mut connection).await;
    let (meeting, _capture) = crate::models::meeting::tests::stored(&mut connection).await;
    let grant = grants
        .grant(
            &CallScope::Session(SessionScope {
                session_id,
                turn_id,
                project_id: Some(project_id),
                action_item_id: None,
                directory: Sessions::directory(session_id),
            }),
            &[],
        )
        .await
        .expect("granted");
    let (sender, events) = mpsc::channel(8);
    let follower = Follower {
        queue: queue.clone(),
        session_id,
        studio_item_id: None,
        project_id: Some(project_id),
        turn_id,
        scrubber: Scrubber::new([]),
        grant,
    };
    let following = tokio::spawn(follower.follow(events));

    // Before it reads a meeting, every client hears what the turn says.
    sender
        .send(RunnerEvent::AgentMessage {
            text: "Looking it up.".to_owned(),
        })
        .await
        .expect("followed");
    let (whole, _payload) = next_heard(&mut heard).await;
    assert_eq!(whole["type"], "session.event");
    assert_eq!(whole["data"]["payload"]["text"], "Looking it up.");

    // A tool keeps what it read before it answers, so what the turn says next is announced by its
    // ids alone, and still recorded whole.
    meeting_reads::keep(
        &mut connection,
        Turn::Session {
            session_id,
            turn_id,
        },
        &[meeting.id],
    )
    .await
    .expect("kept");
    sender
        .send(RunnerEvent::AgentMessage {
            text: "The one-to-one moved the quokka team.".to_owned(),
        })
        .await
        .expect("followed");
    let (held, payload) = next_heard(&mut heard).await;
    assert_eq!(
        held,
        json!({
            "type": "session.eventHeld",
            "data": { "sessionId": session_id, "sequence": 2 },
        })
    );
    assert!(!payload.contains("quokka"), "{payload}");
    let (recorded, _truncated) = session_event::latest(&mut connection, session_id, 10)
        .await
        .expect("history reads");
    assert_eq!(recorded.len(), 2, "both are kept for the history to answer");

    drop(sender);
    following.await.expect("the turn is followed to its end");
}
