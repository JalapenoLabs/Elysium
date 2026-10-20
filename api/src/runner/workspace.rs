// Copyright © 2026 Jalapeno Labs

//! The workspace container every agent turn runs in.
//!
//! Agents never run inside the API's container, which holds the encryption key and the
//! database's credentials: an agent with every permission could read them. They run in one
//! long-lived sibling container the API creates and keeps through the filtered Docker proxy,
//! the way it keeps Stalwart (`crate::mail::stalwart::container`). Everything the API creates
//! for it is labelled [`MANAGED_LABEL`]:
//!
//! | Kind      | Name                | Purpose                                                   |
//! |-----------|---------------------|-----------------------------------------------------------|
//! | volume    | `elysium-workspace` | `/workspace`: repositories, sessions, and `~/.claude`     |
//! | container | `elysium-workspace` | Runs turns; answers as `workspace` on `elysium-agents`    |
//!
//! The container joins `elysium-agents`, a network `compose.yml` defines for the API and the
//! workspace alone, where the API answers as `api` (see [`super::AGENT_API_URL`]). It is a
//! plain bridge, so agents reach the internet for git, packages, and the model provider.
//!
//! The image is the published `elysium-workspace` of the same release as the API. A
//! container running any other image is replaced, keeping its volume, the first time the
//! API needs it, which is at startup, before any turn runs.

use std::collections::HashMap;
use std::time::Duration;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::errors::Error as DockerError;
use bollard::exec::StartExecResults;
use bollard::models::{
    ContainerCreateBody, EndpointSettings, ExecConfig, HostConfig, NetworkingConfig, RestartPolicy,
    RestartPolicyNameEnum, VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, RemoveContainerOptionsBuilder,
};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{Level, event};

pub(super) const CONTAINER_NAME: &str = "elysium-workspace";
const VOLUME_NAME: &str = "elysium-workspace";
const NETWORK_NAME: &str = "elysium-agents";
/// What other containers on [`NETWORK_NAME`] call the workspace.
const NETWORK_ALIAS: &str = "workspace";
const MANAGED_LABEL: (&str, &str) = ("dev.elysium.managed", "workspace");

/// The account turns run as. Claude Code refuses `bypassPermissions` as root, and the image
/// creates this user for it.
pub const AGENT_USER: &str = "agent";

/// How long a control script (killing a turn, reaping old ones, fetching the configuration
/// repository) may take. Generous for a slow `git fetch`; turns themselves are not bounded by
/// it.
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(300);

/// Longest output a control script's report keeps, from its end, where errors are.
const SCRIPT_OUTPUT_TAIL: usize = 4_000;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("Docker: {0}")]
    Docker(#[from] DockerError),
    #[error("the workspace exec did not attach its output")]
    Detached,
    #[error("a workspace script took longer than {} seconds", SCRIPT_TIMEOUT.as_secs())]
    ScriptTimeout,
}

/// One process to run in the workspace.
#[derive(Debug, Clone)]
pub struct ExecCommand {
    pub argv: Vec<String>,
    pub environment: Vec<String>,
    pub user: &'static str,
}

/// What a short control script printed, and its exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptOutput {
    pub exit_code: Option<i64>,
    /// The end of stdout and stderr together.
    pub output: String,
}

/// The workspace container, reached through Docker.
#[derive(Debug, Clone)]
pub struct WorkspaceContainer {
    docker: Docker,
    image: String,
}

impl WorkspaceContainer {
    /// `image` is a full reference such as `jalapenolabs/elysium-workspace:v1.2.0`.
    pub const fn new(docker: Docker, image: String) -> Self {
        Self { docker, image }
    }

    /// The Docker client, for the file operations in `super::files`.
    pub(super) const fn docker(&self) -> &Docker {
        &self.docker
    }

    /// Pulls the image if Docker lacks it, then makes sure the container runs it.
    ///
    /// # Errors
    /// Propagates Docker's refusal, such as an image that does not exist.
    pub async fn ensure_running(&self) -> Result<(), WorkspaceError> {
        self.ensure_image().await?;

        match self.docker.inspect_container(CONTAINER_NAME, None).await {
            Ok(existing) => {
                let current_image = existing
                    .config
                    .and_then(|config| config.image)
                    .unwrap_or_default();
                if current_image != self.image {
                    event!(
                        name: "runner.workspace.replaced",
                        Level::INFO,
                        container.image.previous = %current_image,
                        container.image.name = %self.image,
                        "the workspace runs another image; replacing it and keeping its volume",
                    );
                    let options = RemoveContainerOptionsBuilder::new().force(true).build();
                    self.docker
                        .remove_container(CONTAINER_NAME, Some(options))
                        .await?;
                    return self.create().await;
                }
                let running = existing
                    .state
                    .and_then(|state| state.running)
                    .unwrap_or(false);
                if !running {
                    self.docker.start_container(CONTAINER_NAME, None).await?;
                }
                Ok(())
            }
            Err(error) if has_status(&error, 404) => self.create().await,
            Err(error) => Err(error.into()),
        }
    }

    async fn ensure_image(&self) -> Result<(), WorkspaceError> {
        match self.docker.inspect_image(&self.image).await {
            Ok(_) => return Ok(()),
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            Err(_missing) => {}
        }

        event!(name: "runner.workspace.pull_started", Level::INFO, container.image.name = %self.image, "pulling");
        let (repository, tag) = split_reference(&self.image);
        let options = CreateImageOptionsBuilder::new()
            .from_image(repository)
            .tag(tag)
            .build();
        let mut progress = self.docker.create_image(Some(options), None, None);
        while let Some(update) = progress.next().await {
            update?;
        }
        Ok(())
    }

    async fn create(&self) -> Result<(), WorkspaceError> {
        self.docker
            .create_volume(VolumeCreateRequest {
                name: Some(VOLUME_NAME.to_owned()),
                labels: Some(managed_labels()),
                ..Default::default()
            })
            .await?;
        let options = CreateContainerOptionsBuilder::new()
            .name(CONTAINER_NAME)
            .build();
        self.docker
            .create_container(Some(options), container_spec(&self.image))
            .await?;
        self.docker.start_container(CONTAINER_NAME, None).await?;
        event!(name: "runner.workspace.created", Level::INFO, container.name = CONTAINER_NAME, "created");
        Ok(())
    }

    /// Runs `command`, sending each line it prints to `lines` as it arrives, and returns its
    /// exit status once it ends. A closed `lines` receiver does not stop the process.
    ///
    /// # Errors
    /// Propagates Docker's refusal to start the exec or a broken output stream.
    pub async fn exec(
        &self,
        command: ExecCommand,
        lines: mpsc::Sender<String>,
    ) -> Result<Option<i64>, WorkspaceError> {
        let exec = self
            .docker
            .create_exec(
                CONTAINER_NAME,
                ExecConfig {
                    cmd: Some(command.argv),
                    env: Some(command.environment),
                    user: Some(command.user.to_owned()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await?;
        let StartExecResults::Attached { mut output, .. } =
            self.docker.start_exec(&exec.id, None).await?
        else {
            return Err(WorkspaceError::Detached);
        };

        let mut splitter = LineSplitter::default();
        while let Some(chunk) = output.next().await {
            let bytes = match chunk? {
                LogOutput::StdOut { message }
                | LogOutput::StdErr { message }
                | LogOutput::Console { message } => message,
                LogOutput::StdIn { .. } => continue,
            };
            for line in splitter.push(&bytes) {
                // The receiver going away means nobody is listening, not that the turn
                // should stop; cancellation is how a turn is stopped.
                let _unheard = lines.send(line).await;
            }
        }
        if let Some(line) = splitter.finish() {
            let _unheard = lines.send(line).await;
        }

        let inspected = self.docker.inspect_exec(&exec.id).await?;
        Ok(inspected.exit_code)
    }

    /// Runs a short `bash` script with `arguments` as its positional parameters and returns
    /// what it printed.
    ///
    /// # Errors
    /// Propagates Docker's refusal, or [`WorkspaceError::ScriptTimeout`].
    pub async fn run_script(
        &self,
        user: &'static str,
        script: &str,
        arguments: &[&str],
        environment: Vec<String>,
    ) -> Result<ScriptOutput, WorkspaceError> {
        let mut argv = vec![
            "bash".to_owned(),
            "-c".to_owned(),
            script.to_owned(),
            "elysium-script".to_owned(),
        ];
        argv.extend(arguments.iter().map(|argument| (*argument).to_owned()));
        let (sender, mut receiver) = mpsc::channel::<String>(256);
        let command = ExecCommand {
            argv,
            environment,
            user,
        };

        let run = async {
            let collect = async {
                let mut output = String::new();
                while let Some(line) = receiver.recv().await {
                    output.push_str(&line);
                    output.push('\n');
                }
                output
            };
            let (exit_code, output) = tokio::join!(self.exec(command, sender), collect);
            Ok::<_, WorkspaceError>(ScriptOutput {
                exit_code: exit_code?,
                output: tail(&output, SCRIPT_OUTPUT_TAIL),
            })
        };
        tokio::time::timeout(SCRIPT_TIMEOUT, run)
            .await
            .map_err(|_elapsed| WorkspaceError::ScriptTimeout)?
    }
}

/// Splits a byte stream into lines, holding a partial line (or a UTF-8 sequence cut across
/// chunks) until the rest arrives.
#[derive(Debug, Default)]
struct LineSplitter {
    buffer: Vec<u8>,
}

impl LineSplitter {
    fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            lines.push(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
        }
        lines
    }

    fn finish(self) -> Option<String> {
        if self.buffer.is_empty() {
            return None;
        }
        Some(String::from_utf8_lossy(&self.buffer).into_owned())
    }
}

fn container_spec(image: &str) -> ContainerCreateBody {
    ContainerCreateBody {
        image: Some(image.to_owned()),
        hostname: Some(NETWORK_ALIAS.to_owned()),
        env: Some(vec!["TZ=UTC".to_owned()]),
        labels: Some(managed_labels()),
        host_config: Some(HostConfig {
            binds: Some(vec![format!("{VOLUME_NAME}:/workspace")]),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            network_mode: Some(NETWORK_NAME.to_owned()),
            // Reaps the zombies agents' tools leave behind in a long-lived container.
            init: Some(true),
            ..Default::default()
        }),
        networking_config: Some(NetworkingConfig {
            endpoints_config: Some(HashMap::from([(
                NETWORK_NAME.to_owned(),
                EndpointSettings {
                    aliases: Some(vec![NETWORK_ALIAS.to_owned()]),
                    ..Default::default()
                },
            )])),
        }),
        ..Default::default()
    }
}

fn managed_labels() -> HashMap<String, String> {
    HashMap::from([(MANAGED_LABEL.0.to_owned(), MANAGED_LABEL.1.to_owned())])
}

fn has_status(error: &DockerError, status: u16) -> bool {
    matches!(error, DockerError::DockerResponseServerError { status_code, .. } if *status_code == status)
}

/// Splits `repository:tag`, where the repository may carry a registry host with a port. A
/// reference without a tag names `latest`, as Docker reads it.
fn split_reference(image: &str) -> (&str, &str) {
    match image.rsplit_once(':') {
        Some((repository, tag)) if !tag.contains('/') => (repository, tag),
        _ => (image, "latest"),
    }
}

fn tail(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_owned();
    }
    let mut start = text.len() - limit;
    while !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_split_across_chunks_and_a_trailing_line_is_kept() {
        let mut splitter = LineSplitter::default();
        assert_eq!(splitter.push(b"{\"a\":"), Vec::<String>::new());
        assert_eq!(splitter.push(b"1}\n{\"b\""), vec!["{\"a\":1}".to_owned()]);
        // A multi-byte character cut between chunks is reassembled.
        let snowman = "☃".as_bytes();
        assert_eq!(splitter.push(&snowman[..1]), Vec::<String>::new());
        assert_eq!(
            splitter.push(&[&snowman[1..], b"\n"].concat()),
            vec!["{\"b\"☃".to_owned()]
        );
        assert_eq!(splitter.push(b"tail"), Vec::<String>::new());
        assert_eq!(splitter.finish().as_deref(), Some("tail"));
    }

    #[test]
    fn references_split_into_repository_and_tag() {
        assert_eq!(
            split_reference("jalapenolabs/elysium-workspace:v1.2.0"),
            ("jalapenolabs/elysium-workspace", "v1.2.0")
        );
        assert_eq!(
            split_reference("registry.local:5000/workspace"),
            ("registry.local:5000/workspace", "latest")
        );
    }

    #[test]
    fn the_workspace_publishes_no_ports_and_keeps_its_volume() {
        let spec = container_spec("elysium-workspace:dev");
        let host_config = spec.host_config.expect("host config");
        assert!(host_config.port_bindings.is_none());
        assert_eq!(host_config.privileged, None);
        assert_eq!(
            host_config.binds.as_deref(),
            Some(&["elysium-workspace:/workspace".to_owned()][..])
        );
        assert_eq!(host_config.network_mode.as_deref(), Some(NETWORK_NAME));
    }

    #[test]
    fn a_long_script_output_keeps_its_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
        assert_eq!(tail("aé", 1), "", "never cuts inside a character");
    }
}
