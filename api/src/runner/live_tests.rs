// Copyright © 2026 Jalapeno Labs

//! The runner against a real Docker daemon and workspace image, without a credential.
//!
//! Opt-in: it creates the real `elysium-workspace` container and volume on the daemon behind
//! `RUNNER_TEST_DOCKER_URL` (a `docker-socket-proxy` with `EXEC=1`), on an existing
//! `elysium-agents` network, from `RUNNER_TEST_WORKSPACE_IMAGE`. Without a credential the CLI
//! reports that it is not logged in, which proves the exec, the turn script, the stream
//! parsing, and the outcome end to end without spending any quota.
//!
//! The worker's test fetches a real configuration repository into a worker's own container,
//! `CONFIGURATION_TEST_REPOSITORY`, with `CONFIGURATION_TEST_TOKEN` when it is private, and starts
//! Claude Code on what was adapted.

use bollard::{API_DEFAULT_VERSION, Docker};
use secrecy::SecretString;
use tokio_util::sync::CancellationToken;
use url::Url;
use uuid::Uuid;

use super::configuration::{ConfigurationRepository, ConfigurationSource, Fetched};
use crate::worker_pool::worker::container::Containers;
use super::workspace::{AGENT_USER, SCRIPT_TIMEOUT, WorkspaceContainer};
use super::{
    ConversationId, CredentialSource, Executor, Runner, RunnerEvent, TurnMode, TurnRequest,
    TurnStatus, WorkspacePath,
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
            effort: None,
            mode: TurnMode::Agent,
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

/// Starts Claude Code without a credential in the worker's container and prints the outcome of
/// every hook its session ran, one `name outcome` line each: the CLI reads the settings and runs
/// the session's hooks before it finds it is not signed in.
const HOOKS_SCRIPT: &str = r#"
mkdir -p /workspace/live-test && cd /workspace/live-test || exit 70
claude -p "Say hi" --output-format stream-json --verbose 2>/dev/null \
  | jq -r 'select(.subtype == "hook_response") | "\(.hook_name) \(.outcome)"'
"#;

#[tokio::test]
#[ignore = "needs RUNNER_TEST_DOCKER_URL, RUNNER_TEST_WORKSPACE_IMAGE, and CONFIGURATION_TEST_REPOSITORY"]
async fn a_worker_fetches_its_configuration_and_adapts_it_to_run_there() {
    let (Ok(url), Ok(image), Ok(repository)) = (
        std::env::var("RUNNER_TEST_DOCKER_URL"),
        std::env::var("RUNNER_TEST_WORKSPACE_IMAGE"),
        std::env::var("CONFIGURATION_TEST_REPOSITORY"),
    ) else {
        eprintln!(
            "skipped: set RUNNER_TEST_DOCKER_URL, RUNNER_TEST_WORKSPACE_IMAGE, and \
             CONFIGURATION_TEST_REPOSITORY to run the live configuration test"
        );
        return;
    };
    let docker = Docker::connect_with_http(&url, 600, API_DEFAULT_VERSION).expect("a client");
    let containers = Containers::new(
        docker,
        image,
        &SecretString::from("a deployment for the live configuration test"),
    );
    let batch_id = Uuid::now_v7();
    containers
        .ensure_network(batch_id, &[])
        .await
        .expect("the worker's network");
    let workspace = containers.workspace(batch_id, 1_000_000_000);
    let runner = Runner::with_parts(
        Executor::Workspace(workspace.clone()),
        CredentialSource::Fixed(Vec::new()),
        ConfigurationSource::Fixed(ConfigurationRepository {
            url: Url::parse(&repository).expect("a repository URL"),
            github_credential_id: None,
            token: std::env::var("CONFIGURATION_TEST_TOKEN")
                .ok()
                .map(SecretString::from),
        }),
        crate::activity::Activities::new(),
        CancellationToken::new(),
    );

    let fetched = runner.fetch_configuration().await.expect("the settings read");
    let held = runner.configuration_commit().await.expect("read");
    let hooks = workspace
        .run_script(AGENT_USER, HOOKS_SCRIPT, &[], Vec::new(), SCRIPT_TIMEOUT)
        .await
        .expect("the CLI starts");
    runner.shutdown().await;
    containers.remove(batch_id).await.expect("removed");

    let Fetched::Attempted {
        outcome: Ok(checkout),
        ..
    } = fetched
    else {
        panic!("the repository was not fetched: {fetched:?}");
    };
    eprintln!(
        "fetched {} at {} ({}); left out {}:",
        checkout.repository,
        checkout.commit,
        checkout.subject,
        checkout.skipped.len()
    );
    for skipped in &checkout.skipped {
        eprintln!("  {skipped:?}");
    }
    eprintln!("hooks that ran:\n{}", hooks.output.trim());
    assert_eq!(
        held.as_deref(),
        Some(checkout.commit.as_str()),
        "the worker's volume holds the commit it recorded"
    );
    assert!(
        hooks
            .output
            .lines()
            .all(|line| line.ends_with(" success")),
        "every hook left in runs in the worker"
    );
}
