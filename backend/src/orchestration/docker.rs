//! The Docker [`Provisioner`] backend, talking to a scoped socket-proxy.
//!
//! [`DockerProvisioner`] reconciles [`ServiceSpec`]s against a Docker Engine API
//! reached over plain HTTP. The API process never holds the raw Docker socket:
//! it talks to a [`tecnativa/docker-socket-proxy`] on an internal Docker network
//! that whitelists only the calls the provisioner makes, so a compromise of the
//! backend cannot own the host. The same code drives a rootless Podman
//! Docker-compatible socket unchanged, so no Podman-specific path exists (see
//! `docs/integrations.md`).
//!
//! Two properties are enforced by construction rather than trusted from a spec:
//!
//! - **The bind-mount allow-list.** Every [`VolumeBind::host_source`] must resolve
//!   to a path inside one of the configured `allowed_roots` (the deployment's data
//!   directory), so a spec can never bind-mount `/`, `/etc`, or the Docker socket.
//!   The guard runs inside the pure [`DockerProvisioner::build_container_config`]
//!   translation, before any Docker call.
//! - **Host hardening.** The generated `HostConfig` is never privileged, never
//!   joins a host namespace, adds no capabilities, and mounts only validated
//!   binds. [`ServiceSpec`] carries no field that could request otherwise, so
//!   these escapes are unrepresentable, not merely rejected.
//!
//! Managed containers join the dedicated `elysium-managed` bridge network and
//! carry the Elysium managed label plus a fingerprint of their desired spec.
//! `ensure` compares that fingerprint to decide no-op versus replace, so a
//! re-drive of unchanged desired state launches nothing new.
//!
//! [`tecnativa/docker-socket-proxy`]: https://github.com/Tecnativa/docker-socket-proxy

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use bollard::errors::Error as BollardError;
use bollard::{models, query_parameters, Docker, API_DEFAULT_VERSION};
use sha2::{Digest, Sha256};
use tokio_stream::StreamExt;

use super::provisioner::{
    HealthCheck, PortProtocol, ProvisionError, Provisioner, RestartPolicy as SpecRestartPolicy,
    ServiceHandle, ServiceSpec, ServiceState, VolumeBind,
};

/// The dedicated bridge network every managed container joins.
///
/// Isolates Elysium's services from unrelated host containers; created on demand
/// and shared by all managed containers so they can reach one another by name.
const MANAGED_NETWORK: &str = "elysium-managed";

/// Label key marking a container or network as Elysium-managed.
///
/// An engine-side sweep (`docker ps --filter label=...`) keys on this to find
/// exactly the objects Elysium owns. Distinct from the per-integration labels the
/// reconciler stamps; this one is applied by the backend to everything it creates.
const MANAGED_LABEL: &str = "ai.jalapenolabs.elysium.managed";

/// Label key carrying the desired-spec fingerprint for change detection.
const FINGERPRINT_LABEL: &str = "ai.jalapenolabs.elysium.fingerprint";

/// Per-call read/write timeout for the Docker HTTP transport, in seconds.
///
/// Generous enough for a slow image create on a busy daemon; the proxy and daemon
/// are local, so this only guards a wedged engine, never normal latency.
const CLIENT_TIMEOUT_SECS: u64 = 120;

/// Grace period a container gets to stop before the engine sends `SIGKILL`.
const STOP_TIMEOUT_SECS: i32 = 10;

/// Upper bound on log lines [`Provisioner::logs`] returns.
///
/// A bounded tail for the admin console; live streaming is a later extension of
/// the seam, so this keeps a busy container's history from flooding a response.
const LOG_TAIL_LINES: usize = 200;

/// A [`Provisioner`] backed by the Docker Engine API behind a scoped proxy.
///
/// Holds a bollard client wired to the proxy over HTTP, the canonicalized host
/// roots a bind mount may target, and the managed network name. Cloneable via
/// the `Arc` in [`super::provisioner::ProvisionerHandle`]; the client itself is
/// internally reference counted.
pub struct DockerProvisioner {
    /// The Engine API client, connected to the socket-proxy over HTTP.
    docker: Docker,
    /// Canonicalized host directories a bind mount may resolve inside.
    allowed_roots: Vec<PathBuf>,
    /// Name of the bridge network managed containers attach to.
    network: String,
}

// Hand-written so the client (which need not be `Debug`) never has to render, and
// so no connection detail leaks; only the security-relevant config is shown.
impl std::fmt::Debug for DockerProvisioner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerProvisioner")
            .field("allowed_roots", &self.allowed_roots)
            .field("network", &self.network)
            .finish_non_exhaustive()
    }
}

impl DockerProvisioner {
    /// Connects to the Docker Engine API at `proxy_url` (the scoped socket-proxy).
    ///
    /// `allowed_roots` are the host directories a bind mount may target; each is
    /// canonicalized when it already exists so symlinks resolve once, up front,
    /// and a non-existent root is kept as given (the data directory is created
    /// lazily elsewhere). The client is lazy, like the Postgres pool: a dead
    /// proxy surfaces on the first call, not here.
    ///
    /// # Errors
    /// Fails when a client cannot be constructed for `proxy_url`.
    pub fn connect(proxy_url: &str, allowed_roots: Vec<PathBuf>) -> Result<Self, ProvisionError> {
        let docker = Docker::connect_with_http(proxy_url, CLIENT_TIMEOUT_SECS, API_DEFAULT_VERSION)
            .map_err(|error| {
                ProvisionError::backend_with_source(
                    "docker",
                    format!("could not build a Docker client for {proxy_url:?}"),
                    error,
                )
            })?;
        let allowed_roots = allowed_roots
            .into_iter()
            .map(|root| root.canonicalize().unwrap_or(root))
            .collect();
        Ok(Self {
            docker,
            allowed_roots,
            network: MANAGED_NETWORK.to_string(),
        })
    }

    /// Translates a [`ServiceSpec`] into the container name and create body.
    ///
    /// Pure: validates the spec, runs the bind-mount allow-list guard, and builds
    /// the bollard request without touching the daemon, so the translation and the
    /// guard are unit-tested without a running engine. The env (secrets exposed
    /// only here), binds, port publishings, health check, restart policy, managed
    /// and fingerprint labels, and the managed-network attachment all derive
    /// deterministically from the spec.
    ///
    /// # Errors
    /// Fails when the spec is invalid ([`ServiceSpec::validate`]) or a volume's
    /// host source resolves outside the allowed bind-mount roots.
    fn build_container_config(
        &self,
        spec: &ServiceSpec,
    ) -> Result<(String, models::ContainerCreateBody), ProvisionError> {
        spec.validate()?;

        // Bind-mount allow-list guard, before any Docker call: a rejected spec
        // never reaches the engine.
        let mut binds = Vec::with_capacity(spec.volumes.len());
        for volume in &spec.volumes {
            self.guard_bind(&spec.name, &volume.host_source)?;
            binds.push(bind_string(volume));
        }

        // Env in deterministic (BTreeMap) order; the real secret value is read
        // only here, at the point it is handed to the engine.
        let env: Vec<String> = spec
            .env
            .iter()
            .map(|(key, value)| format!("{key}={}", value.expose()))
            .collect();

        // Ports become both an exposed-port set and host publishings.
        let mut exposed_ports = Vec::with_capacity(spec.ports.len());
        let mut port_bindings: models::PortMap = HashMap::new();
        for port in &spec.ports {
            let key = format!("{}/{}", port.container_port, protocol_str(port.protocol));
            exposed_ports.push(key.clone());
            port_bindings.insert(
                key,
                Some(vec![models::PortBinding {
                    host_ip: None,
                    host_port: Some(port.host_port.to_string()),
                }]),
            );
        }

        // The spec's own labels, then the managed and fingerprint labels added
        // last so a spec can never overwrite them.
        let mut labels: HashMap<String, String> = spec
            .labels
            .iter()
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect();
        labels.insert(MANAGED_LABEL.to_string(), "true".to_string());
        labels.insert(FINGERPRINT_LABEL.to_string(), fingerprint(spec));

        let host_config = models::HostConfig {
            binds: (!binds.is_empty()).then_some(binds),
            port_bindings: (!port_bindings.is_empty()).then_some(port_bindings),
            restart_policy: Some(restart_policy(spec.restart_policy)),
            // Hardening by construction. `ServiceSpec` cannot express any of these
            // escapes, so every dangerous field stays at its safe default; we pin
            // `privileged` off explicitly to make the guarantee testable and
            // obvious to a reader.
            privileged: Some(false),
            ..Default::default()
        };

        let networking_config = models::NetworkingConfig {
            endpoints_config: Some(HashMap::from([(
                self.network.clone(),
                models::EndpointSettings::default(),
            )])),
        };

        let config = models::ContainerCreateBody {
            image: Some(spec.image.clone()),
            env: (!env.is_empty()).then_some(env),
            exposed_ports: (!exposed_ports.is_empty()).then_some(exposed_ports),
            labels: Some(labels),
            healthcheck: spec.health_check.as_ref().map(health_config),
            host_config: Some(host_config),
            networking_config: Some(networking_config),
            ..Default::default()
        };

        Ok((spec.name.clone(), config))
    }

    /// Rejects a bind whose host source resolves outside the allowed roots.
    ///
    /// # Errors
    /// Returns an invalid-spec error naming `service` when `host_source` resolves
    /// outside every allowed root.
    fn guard_bind(&self, service: &str, host_source: &str) -> Result<(), ProvisionError> {
        let resolved = resolve_existing_prefix(Path::new(host_source));
        if self
            .allowed_roots
            .iter()
            .any(|root| resolved.starts_with(root))
        {
            return Ok(());
        }
        Err(ProvisionError::invalid_spec(
            service,
            format!(
                "volume host source {host_source:?} resolves to {} outside the allowed \
                 bind-mount roots {:?}",
                resolved.display(),
                self.allowed_roots
            ),
        ))
    }

    /// Ensures the managed bridge network exists, idempotently.
    ///
    /// # Errors
    /// Fails when the network cannot be inspected or created.
    async fn ensure_network(&self) -> Result<(), ProvisionError> {
        match self
            .docker
            .inspect_network(
                &self.network,
                None::<query_parameters::InspectNetworkOptions>,
            )
            .await
        {
            Ok(_) => Ok(()),
            Err(error) if is_not_found(&error) => {
                let request = models::NetworkCreateRequest {
                    name: self.network.clone(),
                    driver: Some("bridge".to_string()),
                    labels: Some(HashMap::from([(
                        MANAGED_LABEL.to_string(),
                        "true".to_string(),
                    )])),
                    ..Default::default()
                };
                match self.docker.create_network(request).await {
                    Ok(_) => Ok(()),
                    // A concurrent ensure may have created it between our inspect
                    // and create; an already-exists conflict is success.
                    Err(error) if is_conflict(&error) => Ok(()),
                    Err(error) => Err(backend_error(&self.network, "create network", error)),
                }
            }
            Err(error) => Err(backend_error(&self.network, "inspect network", error)),
        }
    }

    /// Creates the container from `config` and starts it, returning its handle.
    ///
    /// # Errors
    /// Fails when the engine cannot create or start the container.
    async fn create_and_start(
        &self,
        name: &str,
        config: models::ContainerCreateBody,
    ) -> Result<ServiceHandle, ProvisionError> {
        let options = query_parameters::CreateContainerOptionsBuilder::default()
            .name(name)
            .build();
        let created = self
            .docker
            .create_container(Some(options), config)
            .await
            .map_err(|error| backend_error(name, "create container", error))?;
        self.docker
            .start_container(name, None::<query_parameters::StartContainerOptions>)
            .await
            .map_err(|error| backend_error(name, "start container", error))?;
        let state = self.state_of(name).await?;
        Ok(ServiceHandle {
            name: name.to_string(),
            id: created.id,
            state,
        })
    }

    /// Force-removes the named container; a no-op when it is already absent.
    ///
    /// # Errors
    /// Fails only when the engine cannot remove an existing container.
    async fn force_remove(&self, name: &str) -> Result<(), ProvisionError> {
        let options = query_parameters::RemoveContainerOptionsBuilder::default()
            .force(true)
            .build();
        match self.docker.remove_container(name, Some(options)).await {
            Ok(()) => Ok(()),
            Err(error) if is_not_found(&error) => Ok(()),
            Err(error) => Err(backend_error(name, "remove container", error)),
        }
    }

    /// Reports the named container's state, [`ServiceState::Absent`] if missing.
    ///
    /// # Errors
    /// Fails only when the engine cannot be queried.
    async fn state_of(&self, name: &str) -> Result<ServiceState, ProvisionError> {
        match self
            .docker
            .inspect_container(name, None::<query_parameters::InspectContainerOptions>)
            .await
        {
            Ok(response) => Ok(response
                .state
                .as_ref()
                .map_or(ServiceState::Absent, map_state)),
            Err(error) if is_not_found(&error) => Ok(ServiceState::Absent),
            Err(error) => Err(backend_error(name, "inspect container", error)),
        }
    }
}

// The trait impl is thin: it orchestrates the inherent helpers above and holds
// the seam's idempotency contract. `async fn` satisfies the trait's
// `impl Future + Send` signatures because every future here is `Send`.
impl Provisioner for DockerProvisioner {
    async fn ensure(&self, spec: &ServiceSpec) -> Result<ServiceHandle, ProvisionError> {
        // Validate, guard, and translate before any Docker call.
        let (name, config) = self.build_container_config(spec)?;
        let desired_fingerprint = fingerprint(spec);

        self.ensure_network().await?;

        match self
            .docker
            .inspect_container(&name, None::<query_parameters::InspectContainerOptions>)
            .await
        {
            Ok(existing) => {
                let current_fingerprint = existing
                    .config
                    .as_ref()
                    .and_then(|config| config.labels.as_ref())
                    .and_then(|labels| labels.get(FINGERPRINT_LABEL));
                if current_fingerprint.map(String::as_str) == Some(desired_fingerprint.as_str()) {
                    // Converged already: report the observed state, change nothing.
                    let state = existing
                        .state
                        .as_ref()
                        .map_or(ServiceState::Absent, map_state);
                    return Ok(ServiceHandle {
                        name,
                        id: existing.id.unwrap_or_default(),
                        state,
                    });
                }
                // The spec changed: replace the container with the new one.
                self.force_remove(&name).await?;
                self.create_and_start(&name, config).await
            }
            Err(error) if is_not_found(&error) => self.create_and_start(&name, config).await,
            Err(error) => Err(backend_error(&name, "inspect during ensure", error)),
        }
    }

    async fn start(&self, name: &str) -> Result<(), ProvisionError> {
        match self.state_of(name).await? {
            // Absent is a not-found error; both running states are a no-op.
            ServiceState::Absent => Err(ProvisionError::not_found(name)),
            ServiceState::Running | ServiceState::Unhealthy => Ok(()),
            ServiceState::Created | ServiceState::Stopped | ServiceState::Failed => self
                .docker
                .start_container(name, None::<query_parameters::StartContainerOptions>)
                .await
                .map_err(|error| backend_error(name, "start container", error)),
        }
    }

    async fn stop(&self, name: &str) -> Result<(), ProvisionError> {
        match self.state_of(name).await? {
            // Absent is a not-found error; every not-running state is a no-op.
            ServiceState::Absent => Err(ProvisionError::not_found(name)),
            ServiceState::Created | ServiceState::Stopped | ServiceState::Failed => Ok(()),
            ServiceState::Running | ServiceState::Unhealthy => {
                let options = query_parameters::StopContainerOptionsBuilder::default()
                    .t(STOP_TIMEOUT_SECS)
                    .build();
                self.docker
                    .stop_container(name, Some(options))
                    .await
                    .map_err(|error| backend_error(name, "stop container", error))
            }
        }
    }

    async fn remove(&self, name: &str) -> Result<(), ProvisionError> {
        self.force_remove(name).await
    }

    async fn inspect(&self, name: &str) -> Result<ServiceState, ProvisionError> {
        self.state_of(name).await
    }

    async fn logs(&self, name: &str) -> Result<String, ProvisionError> {
        // Logs of a nonexistent service is a not-found error (seam contract).
        if self.state_of(name).await? == ServiceState::Absent {
            return Err(ProvisionError::not_found(name));
        }
        let options = query_parameters::LogsOptionsBuilder::default()
            .stdout(true)
            .stderr(true)
            .tail(&LOG_TAIL_LINES.to_string())
            .build();
        let mut stream = std::pin::pin!(self.docker.logs(name, Some(options)));
        let mut output = String::new();
        while let Some(chunk) = stream.next().await {
            match chunk {
                // `LogOutput` renders its payload as UTF-8 (lossy); Docker already
                // frames each line, so concatenating preserves the log text.
                Ok(line) => output.push_str(&line.to_string()),
                Err(error) => return Err(backend_error(name, "read logs", error)),
            }
        }
        Ok(output)
    }
}

/// The `host:container[:ro]` bind string bollard's `HostConfig.Binds` expects.
fn bind_string(volume: &VolumeBind) -> String {
    if volume.read_only {
        format!("{}:{}:ro", volume.host_source, volume.container_path)
    } else {
        format!("{}:{}", volume.host_source, volume.container_path)
    }
}

/// The Docker port-key protocol suffix (`tcp`/`udp`).
fn protocol_str(protocol: PortProtocol) -> &'static str {
    match protocol {
        PortProtocol::Tcp => "tcp",
        PortProtocol::Udp => "udp",
    }
}

/// Translates a spec [`RestartPolicy`](SpecRestartPolicy) to bollard's.
fn restart_policy(policy: SpecRestartPolicy) -> models::RestartPolicy {
    let name = match policy {
        SpecRestartPolicy::Never => models::RestartPolicyNameEnum::NO,
        SpecRestartPolicy::OnFailure => models::RestartPolicyNameEnum::ON_FAILURE,
        SpecRestartPolicy::Always => models::RestartPolicyNameEnum::ALWAYS,
        SpecRestartPolicy::UnlessStopped => models::RestartPolicyNameEnum::UNLESS_STOPPED,
    };
    models::RestartPolicy {
        name: Some(name),
        ..Default::default()
    }
}

/// Translates a [`HealthCheck`] into bollard's container health config.
///
/// Docker's `Test` runs the argv directly when prefixed with `CMD` (versus
/// `CMD-SHELL`, which wraps it in a shell); durations are nanoseconds.
fn health_config(check: &HealthCheck) -> models::HealthConfig {
    let mut test = Vec::with_capacity(check.command.len() + 1);
    test.push("CMD".to_string());
    test.extend(check.command.iter().cloned());
    models::HealthConfig {
        test: Some(test),
        interval: Some(check.interval.as_nanos() as i64),
        timeout: Some(check.timeout.as_nanos() as i64),
        retries: Some(i64::from(check.retries)),
        ..Default::default()
    }
}

/// Maps a bollard container state to a [`ServiceState`].
///
/// Running (or restarting) with a failing health check is [`ServiceState::Unhealthy`];
/// otherwise running is [`ServiceState::Running`]. A clean exit is
/// [`ServiceState::Stopped`], a non-zero or dead exit is [`ServiceState::Failed`],
/// and transient in-between states report the not-running end so the reconciler
/// re-drives rather than trusting them.
fn map_state(state: &models::ContainerState) -> ServiceState {
    use models::ContainerStateStatusEnum as Status;
    match state.status {
        Some(Status::CREATED) => ServiceState::Created,
        Some(Status::RUNNING | Status::RESTARTING) => {
            if health_is_unhealthy(state) {
                ServiceState::Unhealthy
            } else {
                ServiceState::Running
            }
        }
        Some(Status::EXITED) => {
            if state.exit_code.unwrap_or_default() == 0 && !state.dead.unwrap_or(false) {
                ServiceState::Stopped
            } else {
                ServiceState::Failed
            }
        }
        Some(Status::DEAD) => ServiceState::Failed,
        _ => ServiceState::Stopped,
    }
}

/// Whether a container's health probe currently reports unhealthy.
fn health_is_unhealthy(state: &models::ContainerState) -> bool {
    matches!(
        state.health.as_ref().and_then(|health| health.status),
        Some(models::HealthStatusEnum::UNHEALTHY)
    )
}

/// A stable hash identifying the desired container configuration.
///
/// Stored as a label so `ensure` answers "is the running container this spec?"
/// with a single comparison. Hashes the spec, which fully determines the
/// translated config (translation is a pure function of the spec and the
/// provisioner's fixed settings), over its deterministic ordering (`BTreeMap`s
/// and `Vec`s). Exposed env values, secrets included, feed the hash so a rotated
/// credential changes the fingerprint and triggers a replace; the hash is
/// one-way, so no secret material lands in the label.
fn fingerprint(spec: &ServiceSpec) -> String {
    // Field and record separators keep distinct specs from colliding by
    // concatenation (e.g. keys/values that would otherwise run together).
    const FIELD: u8 = 0x1f;
    const RECORD: u8 = 0x1e;
    const SECTION: u8 = 0xff;

    let mut hasher = Sha256::new();
    hasher.update(spec.image.as_bytes());
    hasher.update([SECTION]);
    for (key, value) in &spec.env {
        hasher.update(key.as_bytes());
        hasher.update([FIELD]);
        hasher.update(value.expose().as_bytes());
        hasher.update([RECORD]);
    }
    hasher.update([SECTION]);
    for volume in &spec.volumes {
        hasher.update(volume.host_source.as_bytes());
        hasher.update([FIELD]);
        hasher.update(volume.container_path.as_bytes());
        hasher.update([FIELD]);
        hasher.update([u8::from(volume.read_only)]);
        hasher.update([RECORD]);
    }
    hasher.update([SECTION]);
    for port in &spec.ports {
        hasher.update(port.host_port.to_le_bytes());
        hasher.update(port.container_port.to_le_bytes());
        hasher.update([protocol_byte(port.protocol)]);
        hasher.update([RECORD]);
    }
    hasher.update([SECTION]);
    for network in &spec.networks {
        hasher.update(network.as_bytes());
        hasher.update([RECORD]);
    }
    hasher.update([SECTION]);
    for (key, value) in &spec.labels {
        hasher.update(key.as_bytes());
        hasher.update([FIELD]);
        hasher.update(value.as_bytes());
        hasher.update([RECORD]);
    }
    hasher.update([SECTION]);
    hasher.update([restart_byte(spec.restart_policy)]);
    if let Some(check) = &spec.health_check {
        hasher.update([0x01]);
        for arg in &check.command {
            hasher.update(arg.as_bytes());
            hasher.update([FIELD]);
        }
        hasher.update(check.interval.as_nanos().to_le_bytes());
        hasher.update(check.timeout.as_nanos().to_le_bytes());
        hasher.update(check.retries.to_le_bytes());
    }
    format!("{:x}", hasher.finalize())
}

/// A stable byte tag for a protocol, for the fingerprint hash.
fn protocol_byte(protocol: PortProtocol) -> u8 {
    match protocol {
        PortProtocol::Tcp => 0,
        PortProtocol::Udp => 1,
    }
}

/// A stable byte tag for a restart policy, for the fingerprint hash.
fn restart_byte(policy: SpecRestartPolicy) -> u8 {
    match policy {
        SpecRestartPolicy::Never => 0,
        SpecRestartPolicy::OnFailure => 1,
        SpecRestartPolicy::Always => 2,
        SpecRestartPolicy::UnlessStopped => 3,
    }
}

/// Resolves `path` to a symlink-free absolute path, tolerating a missing tail.
///
/// Bind targets may not exist until the engine creates them on first mount, so a
/// plain [`Path::canonicalize`] would fail. This canonicalizes the longest
/// existing ancestor (following real symlinks once) and folds the remaining
/// components lexically, so a `..` in the tail can never climb above the resolved
/// ancestor. The result is safe to compare against the allowed roots.
fn resolve_existing_prefix(path: &Path) -> PathBuf {
    let mut existing = path;
    while !existing.exists() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => break,
        }
    }
    let base = existing
        .canonicalize()
        .unwrap_or_else(|_| existing.to_path_buf());
    let tail = path.strip_prefix(existing).unwrap_or(path);

    let mut resolved = base;
    for component in tail.components() {
        match component {
            Component::Normal(segment) => resolved.push(segment),
            Component::ParentDir => {
                resolved.pop();
            }
            // A root, prefix, or `.` in the tail carries no new segment.
            Component::RootDir | Component::Prefix(_) | Component::CurDir => {}
        }
    }
    resolved
}

/// Whether a bollard error is the engine's 404 "no such object" response.
fn is_not_found(error: &BollardError) -> bool {
    matches!(
        error,
        BollardError::DockerResponseServerError {
            status_code: 404,
            ..
        }
    )
}

/// Whether a bollard error is the engine's 409 "already exists" conflict.
fn is_conflict(error: &BollardError) -> bool {
    matches!(
        error,
        BollardError::DockerResponseServerError {
            status_code: 409,
            ..
        }
    )
}

/// Wraps a bollard error as a backend [`ProvisionError`], keeping it as the cause.
fn backend_error(service: &str, action: &str, error: BollardError) -> ProvisionError {
    ProvisionError::backend_with_source(service, format!("docker {action} failed"), error)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::orchestration::provisioner::{EnvValue, PortBind};

    /// A provisioner whose single allowed root is `root`, wired to an unused URL.
    ///
    /// The client is lazy, so nothing connects; only the pure translation and the
    /// allow-list guard are exercised here.
    fn provisioner(root: &Path) -> DockerProvisioner {
        DockerProvisioner::connect("http://127.0.0.1:2375", vec![root.to_path_buf()])
            .expect("building a lazy client never fails")
    }

    /// A spec that binds `<root>/data` and publishes a port, with a secret env.
    fn full_spec(root: &Path) -> ServiceSpec {
        let mut spec = ServiceSpec::new("forgejo", "codeberg.org/forgejo/forgejo:13.0.1");
        spec.env
            .insert("PLAIN".to_string(), EnvValue::plain("visible"));
        spec.env
            .insert("SECRET".to_string(), EnvValue::secret("s3cr3t-value"));
        spec.volumes.push(VolumeBind {
            host_source: format!("{}/data", root.display()),
            container_path: "/data".to_string(),
            read_only: false,
        });
        spec.volumes.push(VolumeBind {
            host_source: format!("{}/config", root.display()),
            container_path: "/config".to_string(),
            read_only: true,
        });
        spec.ports.push(PortBind {
            host_port: 3000,
            container_port: 3000,
            protocol: PortProtocol::Tcp,
        });
        spec.labels
            .insert("io.elysium.managed".to_string(), "true".to_string());
        spec
    }

    // --- Translation ---------------------------------------------------------

    #[test]
    fn build_container_config_translates_every_field() {
        let root = TempDir::new().expect("tempdir");
        let provisioner = provisioner(root.path());
        let spec = full_spec(root.path());

        let (name, config) = provisioner
            .build_container_config(&spec)
            .expect("a valid, in-bounds spec translates");

        assert_eq!(name, "forgejo");
        assert_eq!(config.image.as_deref(), Some(spec.image.as_str()));

        // Env carries both entries, and the secret's real value is exposed to the
        // engine (the redaction is only for debug rendering).
        let env = config.env.expect("env present");
        assert!(env.contains(&"PLAIN=visible".to_string()));
        assert!(env.contains(&"SECRET=s3cr3t-value".to_string()));

        let host_config = config.host_config.expect("host config present");
        let binds = host_config.binds.expect("binds present");
        assert!(binds.contains(&format!("{}/data:/data", root.path().display())));
        assert!(binds.contains(&format!("{}/config:/config:ro", root.path().display())));

        // The port is both exposed and published to the same host port.
        assert_eq!(
            config.exposed_ports.as_deref(),
            Some(&["3000/tcp".to_string()][..])
        );
        let publishings = host_config.port_bindings.expect("port bindings present");
        let mapped = publishings
            .get("3000/tcp")
            .expect("port key")
            .as_ref()
            .expect("bindings");
        assert_eq!(mapped[0].host_port.as_deref(), Some("3000"));

        // The default restart policy translates to unless-stopped.
        assert_eq!(
            host_config.restart_policy.and_then(|policy| policy.name),
            Some(models::RestartPolicyNameEnum::UNLESS_STOPPED)
        );

        // The container joins the managed network.
        let endpoints = config
            .networking_config
            .expect("networking")
            .endpoints_config
            .expect("endpoints");
        assert!(endpoints.contains_key(MANAGED_NETWORK));

        // Labels carry the spec's own plus the managed and fingerprint markers.
        let labels = config.labels.expect("labels present");
        assert_eq!(
            labels.get("io.elysium.managed").map(String::as_str),
            Some("true")
        );
        assert_eq!(labels.get(MANAGED_LABEL).map(String::as_str), Some("true"));
        assert_eq!(
            labels.get(FINGERPRINT_LABEL).map(String::as_str),
            Some(fingerprint(&spec).as_str())
        );
    }

    #[test]
    fn generated_host_config_is_hardened() {
        // A spec cannot request an escape; the translated host config proves the
        // dangerous fields stay off and no bind targets the docker socket.
        let root = TempDir::new().expect("tempdir");
        let (_, config) = provisioner(root.path())
            .build_container_config(&full_spec(root.path()))
            .expect("translate");
        let host_config = config.host_config.expect("host config");

        assert_eq!(host_config.privileged, Some(false));
        assert_eq!(host_config.network_mode, None);
        assert_eq!(host_config.cap_add, None);
        assert_eq!(host_config.pid_mode, None);
        assert_eq!(host_config.ipc_mode, None);
        for bind in host_config.binds.unwrap_or_default() {
            assert!(
                !bind.contains("docker.sock"),
                "no bind may mount the docker socket: {bind}"
            );
        }
    }

    #[test]
    fn restart_and_health_translate() {
        let root = TempDir::new().expect("tempdir");
        let provisioner = provisioner(root.path());
        let mut spec = ServiceSpec::new("svc", "redis:7.4.1-alpine");
        spec.restart_policy = SpecRestartPolicy::OnFailure;
        spec.health_check = Some(HealthCheck {
            command: vec!["redis-cli".to_string(), "ping".to_string()],
            interval: Duration::from_secs(5),
            timeout: Duration::from_secs(2),
            retries: 3,
        });

        let (_, config) = provisioner
            .build_container_config(&spec)
            .expect("translate");
        assert_eq!(
            config
                .host_config
                .and_then(|host| host.restart_policy)
                .and_then(|policy| policy.name),
            Some(models::RestartPolicyNameEnum::ON_FAILURE)
        );
        let health = config.healthcheck.expect("health present");
        // The argv is prefixed with CMD so Docker runs it directly (no shell).
        assert_eq!(
            health.test,
            Some(vec![
                "CMD".to_string(),
                "redis-cli".to_string(),
                "ping".to_string()
            ])
        );
        assert_eq!(health.interval, Some(5_000_000_000));
        assert_eq!(health.timeout, Some(2_000_000_000));
        assert_eq!(health.retries, Some(3));
    }

    // --- Allow-list guard ----------------------------------------------------

    #[test]
    fn build_container_config_rejects_a_bind_outside_the_roots() {
        // The host source escapes the allowed root; translation must fail with an
        // invalid-spec error (and, being pure, makes no Docker call).
        let root = TempDir::new().expect("tempdir");
        let provisioner = provisioner(root.path());
        let mut spec = ServiceSpec::new("git", "codeberg.org/forgejo/forgejo:13.0.1");
        spec.volumes.push(VolumeBind {
            host_source: "/etc".to_string(),
            container_path: "/data".to_string(),
            read_only: false,
        });

        let error = provisioner
            .build_container_config(&spec)
            .expect_err("a bind outside the roots is rejected");
        assert!(error.is_invalid_spec());
        assert_eq!(error.service(), "git");
    }

    #[test]
    fn build_container_config_rejects_a_dotdot_escape() {
        // A `..` tail that climbs out of the root is folded and rejected, so a
        // lexical escape cannot slip past the guard.
        let root = TempDir::new().expect("tempdir");
        let provisioner = provisioner(root.path());
        let mut spec = ServiceSpec::new("git", "codeberg.org/forgejo/forgejo:13.0.1");
        spec.volumes.push(VolumeBind {
            host_source: format!("{}/../../etc", root.path().display()),
            container_path: "/data".to_string(),
            read_only: false,
        });

        let error = provisioner
            .build_container_config(&spec)
            .expect_err("a dot-dot escape is rejected");
        assert!(error.is_invalid_spec());
    }

    #[test]
    fn build_container_config_accepts_a_not_yet_created_bind_target() {
        // The reconciler mounts `<root>/.elysium/<kind>/<role>`, which does not
        // exist until first mount; a not-yet-created target inside the root is
        // still allowed.
        let root = TempDir::new().expect("tempdir");
        let provisioner = provisioner(root.path());
        let mut spec = ServiceSpec::new("git", "codeberg.org/forgejo/forgejo:13.0.1");
        spec.volumes.push(VolumeBind {
            host_source: format!("{}/.elysium/git/repos", root.path().display()),
            container_path: "/data".to_string(),
            read_only: false,
        });

        provisioner
            .build_container_config(&spec)
            .expect("a not-yet-created target inside the root is allowed");
    }

    // --- Fingerprint ---------------------------------------------------------

    #[test]
    fn fingerprint_is_stable_and_changes_with_a_rotated_secret() {
        let root = TempDir::new().expect("tempdir");
        let spec = full_spec(root.path());

        // Identical specs fingerprint identically (an unchanged spec is a no-op).
        assert_eq!(fingerprint(&spec), fingerprint(&spec.clone()));

        // Rotating a secret changes the fingerprint, so `ensure` replaces the
        // container on the next reconcile.
        let mut rotated = spec.clone();
        rotated
            .env
            .insert("SECRET".to_string(), EnvValue::secret("rotated-value"));
        assert_ne!(fingerprint(&spec), fingerprint(&rotated));
    }

    #[test]
    fn fingerprint_never_leaks_the_secret_material() {
        // The fingerprint is a one-way hash: the raw secret never appears in it,
        // so the label it becomes is safe to store and surface.
        let root = TempDir::new().expect("tempdir");
        let spec = full_spec(root.path());
        assert!(!fingerprint(&spec).contains("s3cr3t-value"));
    }

    // --- State mapping -------------------------------------------------------

    /// A container state with the given status, exit code, and health.
    fn container_state(
        status: models::ContainerStateStatusEnum,
        exit_code: i64,
        health: Option<models::HealthStatusEnum>,
    ) -> models::ContainerState {
        models::ContainerState {
            status: Some(status),
            exit_code: Some(exit_code),
            health: health.map(|status| models::Health {
                status: Some(status),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn map_state_covers_every_lifecycle_status() {
        use models::ContainerStateStatusEnum as Status;
        use models::HealthStatusEnum as Health;

        assert_eq!(
            map_state(&container_state(Status::CREATED, 0, None)),
            ServiceState::Created
        );
        assert_eq!(
            map_state(&container_state(Status::RUNNING, 0, None)),
            ServiceState::Running
        );
        assert_eq!(
            map_state(&container_state(Status::RUNNING, 0, Some(Health::HEALTHY))),
            ServiceState::Running
        );
        assert_eq!(
            map_state(&container_state(
                Status::RUNNING,
                0,
                Some(Health::UNHEALTHY)
            )),
            ServiceState::Unhealthy
        );
        assert_eq!(
            map_state(&container_state(Status::EXITED, 0, None)),
            ServiceState::Stopped
        );
        assert_eq!(
            map_state(&container_state(Status::EXITED, 137, None)),
            ServiceState::Failed
        );
        assert_eq!(
            map_state(&container_state(Status::DEAD, 0, None)),
            ServiceState::Failed
        );
    }

    // --- Integration (live daemon) ------------------------------------------
    //
    // Runs only when ELYSIUM_DOCKER_PROXY_URL points at a real (proxied) engine;
    // `cargo test` skips it, and the CI runner with Docker exercises it via
    // `cargo test -- --ignored`.

    #[tokio::test]
    #[ignore = "requires a Docker engine via ELYSIUM_DOCKER_PROXY_URL"]
    async fn docker_backend_lifecycle() {
        let Some(proxy_url) = std::env::var("ELYSIUM_DOCKER_PROXY_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            return;
        };

        let root = TempDir::new().expect("tempdir");
        let provisioner = provisioner_for(&proxy_url, root.path());

        // A pinned, long-running image with a default command, no config needed.
        let name = format!("elysium-test-{}", uuid::Uuid::new_v4());
        let spec = ServiceSpec::new(&name, "redis:7.4.1-alpine");

        // Ensure that cleanup runs even if an assertion fails midway.
        let outcome = run_lifecycle(&provisioner, &spec, &name).await;
        provisioner.remove(&name).await.expect("cleanup remove");
        outcome.expect("lifecycle assertions");
    }

    /// A provisioner pointed at a real proxy, for the live test.
    fn provisioner_for(proxy_url: &str, root: &Path) -> DockerProvisioner {
        DockerProvisioner::connect(proxy_url, vec![root.to_path_buf()]).expect("connect")
    }

    /// Drives ensure -> stop -> start -> remove, asserting the observed states.
    async fn run_lifecycle(
        provisioner: &DockerProvisioner,
        spec: &ServiceSpec,
        name: &str,
    ) -> Result<(), String> {
        let check = |actual: ServiceState, expected: ServiceState, step: &str| {
            (actual == expected)
                .then_some(())
                .ok_or_else(|| format!("{step}: expected {expected:?}, got {actual:?}"))
        };

        let handle = provisioner.ensure(spec).await.map_err(|e| e.to_string())?;
        check(handle.state, ServiceState::Running, "ensure")?;
        check(
            provisioner.inspect(name).await.map_err(|e| e.to_string())?,
            ServiceState::Running,
            "inspect after ensure",
        )?;

        provisioner.stop(name).await.map_err(|e| e.to_string())?;
        check(
            provisioner.inspect(name).await.map_err(|e| e.to_string())?,
            ServiceState::Stopped,
            "inspect after stop",
        )?;

        provisioner.start(name).await.map_err(|e| e.to_string())?;
        check(
            provisioner.inspect(name).await.map_err(|e| e.to_string())?,
            ServiceState::Running,
            "inspect after start",
        )?;

        provisioner.remove(name).await.map_err(|e| e.to_string())?;
        check(
            provisioner.inspect(name).await.map_err(|e| e.to_string())?,
            ServiceState::Absent,
            "inspect after remove",
        )
    }
}
