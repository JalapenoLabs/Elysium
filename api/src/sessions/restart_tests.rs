// Copyright © 2026 Jalapeno Labs

//! The sessions queue starting beside the engine's other loops (#192).
//!
//! At engine start, Elysia's dispatcher and the Project Managers' dispatcher start beside the
//! sessions queue, and either may mint a tool grant for its first turn before the queue settles
//! what an earlier engine left. Settling must never take those grants away, or that turn runs
//! without its Elysium tools.

use std::sync::Arc;
use std::time::Duration;

use bollard::{API_DEFAULT_VERSION, Docker};
use diesel::prelude::*;
use diesel_async::RunQueryDsl;
use secrecy::ExposeSecret;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::queue::{TurnQueue, TurnQueueDeps};
use super::{Sessions, SessionsDeps};
use crate::database::Pool;
use crate::database::schema::session_turns;
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::project::{self, NewProject};
use crate::models::session_turn::{self, NewSessionTurn, SessionTurnStatus};
use crate::realtime::EventBus;
use crate::runner::workspace::WorkspaceContainer;
use crate::runner::{McpServer, Runner};
use crate::services::build_passwords;
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};
use crate::tools::grants::{Grant, Grants};
use crate::tools::{CallScope, ElysiaScope, ProjectManagerScope, elysia, project_manager};

/// A sessions queue whose runner is never reached: settling and looking at an empty queue touch
/// only Postgres. Nothing listens on the Docker address, so a call that did reach it would fail.
fn queue(database: &Pool, grants: Grants) -> TurnQueue {
    let cipher = Arc::new(cipher());
    let docker = Docker::connect_with_http("http://127.0.0.1:9", 5, API_DEFAULT_VERSION)
        .expect("a client, which connects only when called");
    let workspace = WorkspaceContainer::new(docker, "elysium-workspace:unused".to_owned());
    let events = EventBus::in_process();
    let runner = Runner::new(
        workspace.clone(),
        database.clone(),
        Arc::clone(&cipher),
        crate::activity::Activities::new(),
        crate::claude_accounts::TurnSignal::default(),
        &CancellationToken::new(),
    );
    let passwords = build_passwords(database.clone(), Arc::clone(&cipher), events.clone());
    TurnQueue::new(TurnQueueDeps {
        sessions: Sessions::new(SessionsDeps {
            database: database.clone(),
            cipher,
            events,
            workspace,
        }),
        runner,
        grants,
        storage: crate::storage::Storage::new(reqwest::Client::new()),
        passwords,
    })
}

/// A session turn whose CLI had started when an earlier engine stopped, which the queue's
/// settling finishes: once it is finished, settling is over.
async fn turn_an_earlier_engine_left(connection: &mut diesel_async::AsyncPgConnection) -> Uuid {
    let project_id = project::create(
        connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Interrupted".to_owned(),
            description: String::new(),
        },
    )
    .await
    .expect("a project")
    .id;
    let id = coding_session::reserve_id(connection)
        .await
        .expect("a number");
    coding_session::create(
        connection,
        &NewCodingSession {
            created_by: TEST_PERSON_ID,
            id,
            project_id: Some(project_id),
            title: "Interrupted".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
        },
    )
    .await
    .expect("a session");
    let turn = session_turn::queue(
        connection,
        &NewSessionTurn {
            id: Uuid::now_v7(),
            session_id: id,
            prompt: "Carry on",
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
    turn.id
}

/// The bearer a grant's turn presents.
fn token(grant: &Grant) -> String {
    let McpServer::Http { headers, .. } = &grant.mcp_servers()[0] else {
        panic!("the tools are served over HTTP");
    };
    headers[0]
        .1
        .expose_secret()
        .strip_prefix("Bearer ")
        .expect("a bearer")
        .to_owned()
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn issue_192_the_sessions_queue_starting_keeps_the_grants_other_loops_just_made() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("Postgres accepts connections");
    let grants = Grants::new(database.clone());
    let interrupted = turn_an_earlier_engine_left(&mut connection).await;

    // The racing order: Elysia's dispatcher and a Project Manager grant their first turns'
    // tools before the sessions queue has settled.
    let elysia_grant = grants
        .grant(
            &CallScope::Elysia(ElysiaScope {
                turn_id: Uuid::now_v7(),
            }),
            &[&elysia::SERVER],
        )
        .await
        .expect("Elysia's grant is stored");
    let manager_grant = grants
        .grant(
            &CallScope::ProjectManager(ProjectManagerScope {
                project_id: Uuid::now_v7(),
                turn_id: Uuid::now_v7(),
            }),
            &[&project_manager::SERVER],
        )
        .await
        .expect("the Project Manager's grant is stored");

    let shutdown = CancellationToken::new();
    let running = queue(&database, grants.clone()).start(shutdown.clone());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        let status: SessionTurnStatus = session_turns::table
            .find(interrupted)
            .select(session_turns::status)
            .first(&mut connection)
            .await
            .expect("the turn reads");
        if status == SessionTurnStatus::Finished {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the queue settled what the earlier engine left in time"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert!(
        grants
            .find(&token(&elysia_grant))
            .await
            .expect("readable")
            .is_some(),
        "Elysia's turn keeps its tools"
    );
    assert!(
        grants
            .find(&token(&manager_grant))
            .await
            .expect("readable")
            .is_some(),
        "the Project Manager's turn keeps its tools"
    );

    shutdown.cancel();
    running.await.expect("the queue stops");
    elysia_grant.revoke().await;
    manager_grant.revoke().await;
}
