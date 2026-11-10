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
//!
//! A worker of the pool runs in a container of its own from the same image
//! ([`WorkspaceContainer::worker`], `crate::worker_pool::worker`): its own volume, network, and
//! CPU limit, named after its batch, so nothing it does reaches the shared workspace or another
//! worker. Everything else here (turns, scripts, files) works the same in either.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bollard::Docker;
use bollard::container::LogOutput;
use bollard::errors::Error as DockerError;
use bollard::exec::StartExecResults;
use bollard::models::{
    ContainerCreateBody, ContainerUpdateBody, EndpointSettings, ExecConfig, HostConfig,
    NetworkingConfig, RestartPolicy, RestartPolicyNameEnum, VolumeCreateRequest,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, CreateImageOptionsBuilder, RemoveContainerOptionsBuilder,
    RemoveVolumeOptionsBuilder,
};
use futures_util::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;
use tracing::{Level, event};

const CONTAINER_NAME: &str = "elysium-workspace";
const VOLUME_NAME: &str = "elysium-workspace";
const NETWORK_NAME: &str = "elysium-agents";
/// What other containers on [`NETWORK_NAME`] call the workspace.
const NETWORK_ALIAS: &str = "workspace";
const MANAGED_LABEL: (&str, &str) = ("dev.elysium.managed", "workspace");

/// The account turns run as. Claude Code refuses `bypassPermissions` as root, and the image
/// creates this user for it.
pub const AGENT_USER: &str = "agent";

/// How long a control script (killing a turn, reaping old ones, fetching the configuration
/// repository, moving a file) may take unless its caller says otherwise. Generous for a slow
/// `git fetch`; turns themselves are not bounded by it.
pub const SCRIPT_TIMEOUT: Duration = Duration::from_mins(5);

/// Longest output a control script's report keeps, from its end, where errors are.
const SCRIPT_OUTPUT_TAIL: usize = 4_000;

#[derive(Debug, thiserror::Error)]
pub enum WorkspaceError {
    #[error("Docker: {0}")]
    Docker(#[from] DockerError),
    #[error("the workspace exec did not attach its output")]
    Detached,
    #[error("a workspace script took longer than {0} seconds")]
    ScriptTimeout(u64),
    #[error("could not hand the process its standard input: {0}")]
    StandardInput(std::io::Error),
}

/// One process to run in the workspace.
#[derive(Debug, Clone)]
pub struct ExecCommand {
    pub argv: Vec<String>,
    pub environment: Vec<String>,
    pub user: &'static str,
    /// Written to the process's standard input, which then closes. `None` attaches none.
    ///
    /// Anything long travels here rather than in `argv` or `environment`: Linux refuses any one
    /// argument or variable longer than 128 KiB (`MAX_ARG_STRLEN`), and every process in the
    /// container can read argv from `ps`.
    pub standard_input: Option<String>,
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
    place: Arc<Place>,
}

/// Which container a [`WorkspaceContainer`] is, which decides its name and how it is created.
#[derive(Debug)]
enum Place {
    /// The one workspace sessions, Project Managers, Elysia, and every step's turn share.
    Shared,
    /// A worker of the pool's own container.
    Worker(WorkerContainer),
}

/// What sets one worker's container apart: its name, which its volume and network share, the
/// labels that say whose it is, and how much of the host's CPU it may use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerContainer {
    /// Shared by the container, its volume, and its network, which must already exist.
    pub name: String,
    pub labels: HashMap<String, String>,
    /// Its CPU limit, in billionths of a CPU, as Docker's `NanoCpus` counts it.
    pub nano_cpus: i64,
}

/// A worker's processes and threads, with room for a parallel build and a desktop, and nothing
/// like a fork bomb.
const WORKER_PIDS_LIMIT: i64 = 8_192;

/// Half Docker's default CPU weight of 1024: under contention the API, the engine, and Postgres
/// get twice the share a worker does, on top of each worker's own limit.
const WORKER_CPU_SHARES: i64 = 512;

impl WorkspaceContainer {
    /// The shared workspace. `image` is a full reference such as
    /// `jalapenolabs/elysium-workspace:v1.2.0`.
    pub fn new(docker: Docker, image: String) -> Self {
        Self {
            docker,
            image,
            place: Arc::new(Place::Shared),
        }
    }

    /// A worker's own container of `image`, as `worker` describes it.
    pub fn worker(docker: Docker, image: String, worker: WorkerContainer) -> Self {
        Self {
            docker,
            image,
            place: Arc::new(Place::Worker(worker)),
        }
    }

    /// The container's name, which execs and scripts run in.
    pub fn name(&self) -> &str {
        match self.place.as_ref() {
            Place::Shared => CONTAINER_NAME,
            Place::Worker(worker) => &worker.name,
        }
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

        match self.docker.inspect_container(self.name(), None).await {
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
                        .remove_container(self.name(), Some(options))
                        .await?;
                    return self.create().await;
                }
                let running = existing
                    .state
                    .and_then(|state| state.running)
                    .unwrap_or(false);
                if !running {
                    self.docker.start_container(self.name(), None).await?;
                }
                Ok(())
            }
            Err(error) if has_status(&error, 404) => self.create().await,
            Err(error) => Err(error.into()),
        }
    }

    /// Whether the container runs now. The API asks before it works in the workspace, which
    /// only the engine creates and replaces ([`Self::ensure_running`]): two processes doing so
    /// at once would race.
    ///
    /// # Errors
    /// Propagates Docker's refusal, other than the container not existing.
    pub async fn is_running(&self) -> Result<bool, WorkspaceError> {
        match self.docker.inspect_container(self.name(), None).await {
            Ok(existing) => Ok(existing
                .state
                .and_then(|state| state.running)
                .unwrap_or(false)),
            Err(error) if has_status(&error, 404) => Ok(false),
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
        // Creating a volume that exists answers the one there, so a replaced container keeps it.
        let (volume, labels) = match self.place.as_ref() {
            Place::Shared => (VOLUME_NAME, managed_labels()),
            Place::Worker(worker) => (worker.name.as_str(), worker.labels.clone()),
        };
        self.docker
            .create_volume(VolumeCreateRequest {
                name: Some(volume.to_owned()),
                labels: Some(labels),
                ..Default::default()
            })
            .await?;
        let options = CreateContainerOptionsBuilder::new()
            .name(self.name())
            .build();
        let spec = match self.place.as_ref() {
            Place::Shared => container_spec(&self.image),
            Place::Worker(worker) => worker_spec(&self.image, worker),
        };
        self.docker.create_container(Some(options), spec).await?;
        self.docker.start_container(self.name(), None).await?;
        event!(name: "runner.workspace.created", Level::INFO, container.name = self.name(), "created");
        Ok(())
    }

    /// Changes a running worker's CPU limit to `nano_cpus` billionths of a CPU, in place.
    ///
    /// # Errors
    /// Propagates Docker's refusal, such as a container that does not exist.
    pub async fn set_cpu_limit(&self, nano_cpus: i64) -> Result<(), WorkspaceError> {
        self.docker
            .update_container(
                self.name(),
                ContainerUpdateBody {
                    nano_cpus: Some(nano_cpus),
                    ..Default::default()
                },
            )
            .await?;
        Ok(())
    }

    /// Removes a worker's container and its volume, once its batch ended. What is already gone
    /// is skipped, so a half-removed worker can be removed again. The shared workspace is never
    /// removed.
    ///
    /// # Errors
    /// Propagates Docker's refusal.
    ///
    /// # Panics
    /// Called on the shared workspace, which is a programming error.
    pub async fn remove_with_volume(&self) -> Result<(), WorkspaceError> {
        let Place::Worker(worker) = self.place.as_ref() else {
            panic!("only a worker's container is ever removed");
        };
        let options = RemoveContainerOptionsBuilder::new().force(true).build();
        match self
            .docker
            .remove_container(&worker.name, Some(options))
            .await
        {
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            _removed_or_missing => {}
        }
        let options = RemoveVolumeOptionsBuilder::new().force(true).build();
        match self.docker.remove_volume(&worker.name, Some(options)).await {
            Err(error) if !has_status(&error, 404) => return Err(error.into()),
            _removed_or_missing => {}
        }
        Ok(())
    }

    /// Runs `command`, sending each line it prints to `lines` as it arrives, and returns its
    /// exit status once it ends. A closed `lines` receiver does not stop the process.
    ///
    /// Its standard input, when it has one, is written while its output is read rather than
    /// before: a process that prints as it reads would otherwise fill its output buffer and stop
    /// reading, and neither side would move.
    ///
    /// # Errors
    /// Propagates Docker's refusal to start the exec or a broken output stream, and
    /// [`WorkspaceError::StandardInput`] when its standard input could not be written whole, once
    /// its output has ended: a process handed part of its input must not pass for one that read
    /// it all.
    pub async fn exec(
        &self,
        command: ExecCommand,
        lines: mpsc::Sender<String>,
    ) -> Result<Option<i64>, WorkspaceError> {
        let exec = self
            .docker
            .create_exec(
                self.name(),
                ExecConfig {
                    cmd: Some(command.argv),
                    env: Some(command.environment),
                    user: Some(command.user.to_owned()),
                    attach_stdin: Some(command.standard_input.is_some()),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await?;
        let StartExecResults::Attached {
            mut output,
            mut input,
        } = self.docker.start_exec(&exec.id, None).await?
        else {
            return Err(WorkspaceError::Detached);
        };

        let writing = async {
            let Some(standard_input) = command.standard_input else {
                return Ok(());
            };
            input.write_all(standard_input.as_bytes()).await?;
            // Closing our half is the end of file the process reads up to.
            input.shutdown().await
        };
        let reading = async {
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
            Ok::<_, WorkspaceError>(())
        };
        let (written, read) = tokio::join!(writing, reading);
        read?;
        written.map_err(WorkspaceError::StandardInput)?;

        let inspected = self.docker.inspect_exec(&exec.id).await?;
        Ok(inspected.exit_code)
    }

    /// Runs a `bash` script with `arguments` as its positional parameters and returns what it
    /// printed, giving up after `timeout`.
    ///
    /// # Errors
    /// Propagates Docker's refusal, or [`WorkspaceError::ScriptTimeout`].
    pub async fn run_script(
        &self,
        user: &'static str,
        script: &str,
        arguments: &[&str],
        environment: Vec<String>,
        timeout: Duration,
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
            standard_input: None,
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
        tokio::time::timeout(timeout, run)
            .await
            .map_err(|_elapsed| WorkspaceError::ScriptTimeout(timeout.as_secs()))?
    }
}

/// Splits a byte stream into lines, holding a partial line (or a UTF-8 sequence cut across
/// chunks) until the rest arrives. Shared with `crate::whisper`, which reads a container's
/// standard error the same way.
#[derive(Debug, Default)]
pub(crate) struct LineSplitter {
    buffer: Vec<u8>,
}

impl LineSplitter {
    pub(crate) fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(bytes);
        let mut lines = Vec::new();
        while let Some(newline) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=newline).collect();
            lines.push(String::from_utf8_lossy(&line[..line.len() - 1]).into_owned());
        }
        lines
    }

    pub(crate) fn finish(self) -> Option<String> {
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

/// A worker's container: the workspace's, on its own volume and network, with no alias, under
/// its CPU limit and weight and a process limit. Like the workspace, it publishes nothing.
fn worker_spec(image: &str, worker: &WorkerContainer) -> ContainerCreateBody {
    ContainerCreateBody {
        image: Some(image.to_owned()),
        hostname: Some("worker".to_owned()),
        env: Some(vec!["TZ=UTC".to_owned()]),
        labels: Some(worker.labels.clone()),
        host_config: Some(HostConfig {
            binds: Some(vec![format!("{}:/workspace", worker.name)]),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            network_mode: Some(worker.name.clone()),
            init: Some(true),
            nano_cpus: Some(worker.nano_cpus),
            cpu_shares: Some(WORKER_CPU_SHARES),
            pids_limit: Some(WORKER_PIDS_LIMIT),
            ..Default::default()
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
/// reference without a tag names `latest`, as Docker reads it. `crate::whisper` pulls its
/// image by the same rule.
pub(crate) fn split_reference(image: &str) -> (&str, &str) {
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
    fn a_worker_runs_on_its_own_volume_and_network_under_its_limits() {
        let worker = WorkerContainer {
            name: "elysium-worker-1".to_owned(),
            labels: HashMap::from([("dev.elysium.managed".to_owned(), "worker".to_owned())]),
            nano_cpus: 1_500_000_000,
        };
        let spec = worker_spec("elysium-workspace:dev", &worker);
        let host_config = spec.host_config.expect("host config");
        assert!(host_config.port_bindings.is_none());
        assert_eq!(host_config.privileged, None);
        assert_eq!(
            host_config.binds.as_deref(),
            Some(&["elysium-worker-1:/workspace".to_owned()][..]),
            "never the shared workspace's volume"
        );
        assert_eq!(
            host_config.network_mode.as_deref(),
            Some("elysium-worker-1")
        );
        assert_eq!(host_config.nano_cpus, Some(1_500_000_000));
        assert_eq!(host_config.cpu_shares, Some(WORKER_CPU_SHARES));
        assert_eq!(host_config.pids_limit, Some(WORKER_PIDS_LIMIT));
        assert!(
            spec.networking_config.is_none(),
            "no alias on any shared network"
        );
    }

    #[test]
    fn a_long_script_output_keeps_its_end() {
        assert_eq!(tail("abcdef", 3), "def");
        assert_eq!(tail("ab", 3), "ab");
        assert_eq!(tail("aé", 1), "", "never cuts inside a character");
    }
}
