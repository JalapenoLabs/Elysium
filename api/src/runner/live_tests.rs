// Copyright © 2026 Jalapeno Labs

//! The runner against a real Docker daemon and workspace image, without a credential.
//!
//! Opt-in: it creates the real `elysium-workspace` container and volume on the daemon behind
//! `RUNNER_TEST_DOCKER_URL` (a `docker-socket-proxy` with `EXEC=1`), on an existing
//! `elysium-agents` network, from `RUNNER_TEST_WORKSPACE_IMAGE`. Without a credential the CLI
//! reports that it is not logged in, which proves the exec, the turn script, the stream
//! parsing, and the outcome end to end without spending any quota.

use bollard::{API_DEFAULT_VERSION, Docker};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::configuration::ConfigurationSource;
use super::workspace::WorkspaceContainer;
use super::{
    ConversationId, CredentialSource, Executor, Runner, RunnerEvent, TurnRequest, TurnStatus,
    WorkspacePath,
};

#[tokio::test]
#[ignore = "needs RUNNER_TEST_DOCKER_URL and RUNNER_TEST_WORKSPACE_IMAGE; creates real containers"]
async fn a_turn_without_a_credential_reports_that_it_is_not_logged_in() {
    // `verify-migrations.sh` runs every ignored test; this one needs a daemon it may not have.
    let (Ok(url), Ok(image)) = (
        std::env::var("RUNNER_TEST_DOCKER_URL"),
        std::env::var("RUNNER_TEST_WORKSPACE_IMAGE"),
    ) else {
        eprintln!(
            "skipped: set RUNNER_TEST_DOCKER_URL and RUNNER_TEST_WORKSPACE_IMAGE to run the live runner test"
        );
        return;
    };
    let docker = Docker::connect_with_http(&url, 600, API_DEFAULT_VERSION).expect("a client");
    let runner = Runner::with_parts(
        Executor::Workspace(WorkspaceContainer::new(docker, image)),
        CredentialSource::Fixed(Vec::new()),
        ConfigurationSource::None,
        crate::activity::Activities::new(),
        CancellationToken::new(),
    );

    let mut ticket = runner
        .submit(TurnRequest {
            conversation: ConversationId::new(Uuid::now_v7()),
            label: "live-test".to_owned(),
            prompt: "Say hi".to_owned(),
            working_directory: WorkspacePath::new("live-test").expect("a path"),
            resume: None,
            mcp_servers: Vec::new(),
            allowed_tools: None,
            system_prompt_append: None,
            environment: Vec::new(),
            model: None,
        })
        .expect("accepted");

    let mut kinds = Vec::new();
    let outcome = loop {
        match ticket.events.recv().await.expect("the turn finishes") {
            RunnerEvent::Finished(outcome) => break outcome,
            event => kinds.push(event),
        }
    };
    assert!(matches!(
        kinds.first(),
        Some(RunnerEvent::TurnStarted { .. })
    ));
    assert_eq!(outcome.status, TurnStatus::Failed, "{outcome:?}");
    assert!(
        outcome.error.unwrap_or_default().contains("Not logged in"),
        "the CLI ran and said why"
    );
    assert!(outcome.harness_session_id.is_some());
}
