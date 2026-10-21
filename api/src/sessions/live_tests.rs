// Copyright © 2026 Jalapeno Labs

//! A session end to end against a real Docker daemon, Postgres, and the workspace image,
//! without a model credential: its repository is cloned into its directory, a turn runs the
//! CLI, and every event the turn reported is recorded in order, ending with its outcome.
//!
//! Opt-in, like `crate::runner`'s live test: it needs `RUNNER_TEST_DOCKER_URL` (a
//! `docker-socket-proxy` with `EXEC=1`), `RUNNER_TEST_WORKSPACE_IMAGE`, an `elysium-agents`
//! network, and `TEST_DATABASE_URL`, and it creates the real `elysium-workspace` container.

use std::sync::Arc;
use std::time::Duration;

use bollard::{API_DEFAULT_VERSION, Docker};
use tokio_util::sync::CancellationToken;

use super::{RepositoryClone, Sessions, SessionsDeps, TurnExtras};
use crate::models::coding_session::{self, NewCodingSession};
use crate::models::project::{self, NewProject};
use crate::models::session_event;
use crate::realtime::EventBus;
use crate::runner::workspace::WorkspaceContainer;
use crate::runner::{Runner, RunnerEvent, TurnStatus};
use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};
use crate::tools::grants::Grants;

#[tokio::test]
#[ignore = "needs RUNNER_TEST_DOCKER_URL, RUNNER_TEST_WORKSPACE_IMAGE, and TEST_DATABASE_URL"]
async fn a_session_clones_runs_a_turn_and_records_every_event() {
    let (Ok(docker_url), Ok(image)) = (
        std::env::var("RUNNER_TEST_DOCKER_URL"),
        std::env::var("RUNNER_TEST_WORKSPACE_IMAGE"),
    ) else {
        eprintln!("skipped: set RUNNER_TEST_DOCKER_URL and RUNNER_TEST_WORKSPACE_IMAGE");
        return;
    };
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let cipher = Arc::new(cipher());
    let docker =
        Docker::connect_with_http(&docker_url, 600, API_DEFAULT_VERSION).expect("a client");
    let workspace = WorkspaceContainer::new(docker, image);
    let runner = Runner::new(
        workspace.clone(),
        database.clone(),
        Arc::clone(&cipher),
        crate::activity::Activities::new(),
        &CancellationToken::new(),
    );
    let passwords =
        crate::server::build_passwords(database.clone(), Arc::clone(&cipher), EventBus::new());
    let sessions = Sessions::new(SessionsDeps {
        database: database.clone(),
        cipher,
        events: EventBus::new(),
        runner,
        workspace,
        grants: Grants::new(),
        storage: crate::storage::Storage::new(reqwest::Client::new()),
        passwords,
    });

    let session = live_session(&mut connection).await;

    sessions
        .prepare(
            &session,
            &[RepositoryClone {
                directory: "hello-world".to_owned(),
                url: "https://github.com/octocat/Hello-World.git".to_owned(),
                base_branch: None,
            }],
        )
        .await
        .expect("a public repository clones");
    let readme = sessions
        .workspace()
        .list_files(&Sessions::directory(session.id), "repos/hello-world")
        .await
        .expect("the clone lists");
    assert!(
        readme.iter().any(|file| file.path == "README"),
        "{readme:?}"
    );

    sessions
        .start_turn(&session, "Say hi".to_owned(), TurnExtras::default())
        .await
        .expect("the turn queues");

    // The follower records the outcome last; wait for it.
    let deadline = tokio::time::Instant::now() + Duration::from_mins(2);
    let recorded = loop {
        let (events, _truncated) = session_event::latest(&mut connection, session.id, 100)
            .await
            .expect("history reads");
        if matches!(
            events.last().map(|stored| &stored.event),
            Some(RunnerEvent::Finished(_))
        ) {
            break events;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the turn finished in time"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    };

    let sequences: Vec<u64> = recorded.iter().map(|stored| stored.sequence).collect();
    let expected: Vec<u64> = (1..=sequences.len() as u64).collect();
    assert_eq!(sequences, expected, "numbered from one without gaps");
    assert!(matches!(recorded[0].event, RunnerEvent::TurnStarted { .. }));
    let Some(RunnerEvent::Finished(outcome)) = recorded.last().map(|stored| &stored.event) else {
        panic!("the outcome is last");
    };
    assert_eq!(outcome.status, TurnStatus::Failed);
    assert!(
        outcome
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("Not logged in")
    );

    sessions.remove(session.id).await;
    let after = sessions
        .workspace()
        .list_files(&Sessions::directory(session.id), "repos")
        .await
        .expect("listing a removed directory lists nothing");
    assert!(after.is_empty());
}

/// A Coding session for a new project, recorded the way the create route records one.
async fn live_session(
    connection: &mut diesel_async::AsyncPgConnection,
) -> crate::models::coding_session::CodingSession {
    let project_id = project::create(
        connection,
        &NewProject {
            created_by: TEST_PERSON_ID,
            name: "Live".to_owned(),
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
            title: "Live".to_owned(),
            github_credential_id: None,
            action_item_id: None,
            studio_item_id: None,
        },
    )
    .await
    .expect("a session")
}
