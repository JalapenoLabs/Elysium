//! The backend-agnostic [`Provisioner`] trait and its declarative service model.
//!
//! A managed service is a [`ServiceSpec`]: a pinned image plus the env, volumes,
//! ports, networks, health check, labels, and restart policy it runs with. A
//! provisioner backend reconciles specs against a real engine; this module owns
//! only the seam, so no engine type (bollard, kube-rs) ever appears in these
//! signatures (`docs/rust.md` M-DONT-LEAK-TYPES).
//!
//! The trait is deliberately a trait rather than a dispatch enum: several real
//! backends are planned (Docker first, then Podman and Kubernetes), and consumers
//! such as the reconciler stay generic (`fn reconcile(p: &impl Provisioner)`),
//! the middle rung of the M-DI-HIERARCHY escalation ladder. No `dyn` is needed;
//! if a single non-generic handle is ever required, a concrete dispatch enum can
//! wrap the backends without changing this seam.
//!
//! Every operation is idempotent: [`Provisioner::ensure`] converges to the spec
//! (a repeat call with an unchanged spec is a no-op), [`Provisioner::remove`] of
//! an absent service succeeds, and [`Provisioner::inspect`] reports absence as
//! [`ServiceState::Absent`] rather than an error. The reconciler can therefore
//! re-drive the full desired state on every boot.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::collections::BTreeMap;
use std::future::Future;
use std::time::Duration;

/// The desired shape of one managed service, declaratively.
///
/// A spec is plain data: cheap to build, compare, and store. Maps are `BTreeMap`
/// so iteration order is deterministic, which keeps spec equality (the
/// idempotency check) and reconcile diffs stable. Construct with
/// [`ServiceSpec::new`] and set the remaining public fields directly; a spec is
/// a data record, not a service, so it carries no builder ceremony.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceSpec {
    /// Unique service name within the deployment; also the container name.
    pub name: String,
    /// Image reference with a pinned tag or digest, e.g. `forgejo:13.0.1` or
    /// `forgejo@sha256:...`. A floating or missing tag fails [`validate`]; the
    /// project pins versions everywhere, and `latest` would make the reconciler's
    /// "is the running container this spec?" question unanswerable.
    ///
    /// [`validate`]: ServiceSpec::validate
    pub image: String,
    /// Environment variables injected into the container. Values are
    /// [`EnvValue`]s so a secret entry never leaks through a debug-formatted
    /// spec; backends read the real value with [`EnvValue::expose`].
    pub env: BTreeMap<String, EnvValue>,
    /// Host paths bound into the container (see [`VolumeBind`]).
    pub volumes: Vec<VolumeBind>,
    /// Ports published from the container to the host (see [`PortBind`]).
    pub ports: Vec<PortBind>,
    /// Names of the networks the service joins; the backend creates them as
    /// needed.
    pub networks: Vec<String>,
    /// Optional liveness probe; without one a service is never `Unhealthy`.
    pub health_check: Option<HealthCheck>,
    /// Labels stamped onto the container, e.g. to mark it Elysium-managed.
    pub labels: BTreeMap<String, String>,
    /// What the engine does when the service exits (see [`RestartPolicy`]).
    pub restart_policy: RestartPolicy,
}

impl ServiceSpec {
    /// Creates a spec for `name` running `image`, with everything else empty.
    pub fn new(name: impl Into<String>, image: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            image: image.into(),
            env: BTreeMap::new(),
            volumes: Vec::new(),
            ports: Vec::new(),
            networks: Vec::new(),
            health_check: None,
            labels: BTreeMap::new(),
            restart_policy: RestartPolicy::default(),
        }
    }

    /// Validates the backend-independent invariants every spec must uphold.
    ///
    /// Backends call this at the top of `ensure`, so the rules hold no matter
    /// which engine runs the service: the name is a valid container name, the
    /// image carries a pinned tag or digest (never floating `latest`), volume
    /// paths are absolute POSIX paths (matching the storage module's rule; the
    /// deployment host is POSIX), ports are non-zero, and a health check has a
    /// command to run.
    ///
    /// # Errors
    /// Returns a [`ProvisionError`] answering `true` to
    /// [`is_invalid_spec`](ProvisionError::is_invalid_spec) naming the first
    /// violated rule.
    pub fn validate(&self) -> Result<(), ProvisionError> {
        let fail = |message: String| Err(ProvisionError::invalid_spec(&self.name, message));

        if !valid_service_name(&self.name) {
            return fail(format!(
                "name {:?} must start with an alphanumeric and contain only [a-zA-Z0-9._-]",
                self.name
            ));
        }
        if !image_is_pinned(&self.image) {
            return fail(format!(
                "image {:?} must carry a pinned tag or digest (floating `latest` is not allowed)",
                self.image
            ));
        }
        for volume in &self.volumes {
            if !volume.host_source.starts_with('/') {
                return fail(format!(
                    "volume host source {:?} must be an absolute path",
                    volume.host_source
                ));
            }
            if !volume.container_path.starts_with('/') {
                return fail(format!(
                    "volume container path {:?} must be an absolute path",
                    volume.container_path
                ));
            }
        }
        for port in &self.ports {
            if port.host_port == 0 || port.container_port == 0 {
                return fail("port bindings must use non-zero ports".to_string());
            }
        }
        if let Some(health_check) = &self.health_check {
            if health_check.command.is_empty() {
                return fail("a health check must have a command to run".to_string());
            }
        }
        Ok(())
    }
}

/// One environment variable value, tagged plain or secret.
///
/// A [`ServiceSpec`] carries admin passwords and minted tokens to the engine,
/// and the same spec flows through reconcile logging and error paths. Tagging
/// each value at construction makes the safe rendering automatic: `Debug` on a
/// secret value (and therefore on any spec containing one) prints `<redacted>`
/// instead of the material, mirroring the [`crate::crypto::Crypto`] debug
/// redaction. There is deliberately no `Display` impl; the only way to read
/// the real value is the explicit [`EnvValue::expose`] call a backend makes
/// when it hands the env to the engine.
#[derive(Clone, PartialEq, Eq)]
pub struct EnvValue {
    value: String,
    secret: bool,
}

impl EnvValue {
    /// A non-sensitive value; `Debug` renders it as-is.
    pub fn plain(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            secret: false,
        }
    }

    /// A sensitive value; `Debug` renders `<redacted>` in its place.
    pub fn secret(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            secret: true,
        }
    }

    /// The real value, for handing to the container engine.
    pub fn expose(&self) -> &str {
        &self.value
    }

    /// Whether this value is redacted from debug output.
    pub fn is_secret(&self) -> bool {
        self.secret
    }
}

// Redacts secret values: the debug output of a spec must never carry secret
// material, even by accident (see the `env_value_debug_redacts_secrets` test).
impl std::fmt::Debug for EnvValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.secret {
            f.write_str("<redacted>")
        } else {
            std::fmt::Debug::fmt(&self.value, f)
        }
    }
}

/// Whether `name` is a portable container/service name.
///
/// The intersection of what the planned engines accept: an alphanumeric first
/// character followed by alphanumerics, `-`, `_`, or `.` (Docker's container
/// name rule, also a valid Kubernetes-safe subset once lowercased).
fn valid_service_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Whether an image reference is pinned to a digest or a non-`latest` tag.
///
/// A digest form (`image@sha256:...`) is always pinned. Otherwise the tag is
/// whatever follows the `:` in the reference's last path segment, so a registry
/// port (`registry:5000/image`) is not mistaken for a tag.
fn image_is_pinned(image: &str) -> bool {
    if let Some((_, digest)) = image.split_once('@') {
        return !digest.is_empty();
    }
    let last_segment = image.rsplit('/').next().unwrap_or(image);
    match last_segment.split_once(':') {
        Some((_, tag)) => !tag.is_empty() && tag != "latest",
        None => false,
    }
}

/// One host path bound into a service's container.
///
/// `host_source` is resolved by the caller from a capability-eligible Drive
/// before it reaches the provisioner (the storage-role binding, #28); by the
/// time a spec exists it is a plain absolute path on the deployment host.
/// Paths are `String`s with the leading-`/` rule, matching the storage module:
/// the deployment host is POSIX, and this keeps validation deterministic across
/// dev platforms rather than deferring to `Path::is_absolute`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolumeBind {
    /// Absolute path on the deployment host.
    pub host_source: String,
    /// Absolute mount point inside the container.
    pub container_path: String,
    /// Mount read-only when the service must not write (e.g. a config bind).
    pub read_only: bool,
}

/// One port published from a service's container to the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortBind {
    /// Port bound on the deployment host.
    pub host_port: u16,
    /// Port the service listens on inside the container.
    pub container_port: u16,
    /// Transport protocol of the binding.
    pub protocol: PortProtocol,
}

/// Transport protocol of a [`PortBind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PortProtocol {
    /// TCP, the default for every planned managed service.
    #[default]
    Tcp,
    /// UDP, for the odd service that needs it.
    Udp,
}

/// A liveness probe the engine runs inside the container.
///
/// A service whose probe keeps failing is reported as
/// [`ServiceState::Unhealthy`], which the reconciler treats as "needs
/// attention", distinct from a clean [`ServiceState::Stopped`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthCheck {
    /// Command executed inside the container; exit 0 means healthy.
    pub command: Vec<String>,
    /// Time between probe runs.
    pub interval: Duration,
    /// How long one probe may take before it counts as failed.
    pub timeout: Duration,
    /// Consecutive failures before the service is considered unhealthy.
    pub retries: u32,
}

/// What the engine does when a managed service's process exits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartPolicy {
    /// Never restart; for one-shot jobs.
    Never,
    /// Restart only after a non-zero exit.
    OnFailure,
    /// Restart on any exit, including engine daemon restarts.
    Always,
    /// Restart on any exit unless explicitly stopped. The default: managed
    /// services are long-running and must survive a host reboot without waiting
    /// for the reconciler, but an admin's explicit stop must stick.
    #[default]
    UnlessStopped,
}

/// The observed lifecycle state of a managed service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// No such service exists on the backend.
    Absent,
    /// Created but never started.
    Created,
    /// Up and, if it has a health check, passing it.
    Running,
    /// Not running: stopped explicitly or exited cleanly.
    Stopped,
    /// Running but failing its health check.
    Unhealthy,
    /// Exited abnormally and not restarting.
    Failed,
}

/// A provisioner's receipt for a reconciled service.
///
/// Carries the canonical name, the backend-native identifier as an opaque
/// string (a Docker container id, a Kubernetes object name), and the state
/// observed right after the operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceHandle {
    /// The service's unique name, as in [`ServiceSpec::name`].
    pub name: String,
    /// Backend-native identifier, opaque to the control plane.
    pub id: String,
    /// State observed when the handle was issued.
    pub state: ServiceState,
}

/// Why a category of provisioning failure occurred.
///
/// Private on purpose (M-ERRORS-CANONICAL-STRUCTS): callers branch through the
/// `is_xxx` accessors on [`ProvisionError`], so internal failure modes can be
/// added without breaking them.
#[derive(Debug)]
enum ErrorKind {
    /// The spec violated a backend-independent invariant (see
    /// [`ServiceSpec::validate`]).
    InvalidSpec { message: String },
    /// The named service does not exist on the backend.
    NotFound,
    /// The backend or its transport failed.
    Backend {
        message: String,
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },
}

/// A provisioning operation failed.
///
/// The canonical error struct for the orchestration seam: a kind, the service
/// name the operation targeted, and a backtrace captured at construction
/// (populated only when `RUST_BACKTRACE` asks for it).
#[derive(Debug)]
pub struct ProvisionError {
    kind: ErrorKind,
    service: String,
    backtrace: Backtrace,
}

impl ProvisionError {
    /// A spec for `service` violated an invariant described by `message`.
    pub(crate) fn invalid_spec(service: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::InvalidSpec {
                message: message.into(),
            },
            service: service.into(),
            backtrace: Backtrace::capture(),
        }
    }

    /// No service named `service` exists on the backend.
    pub(crate) fn not_found(service: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::NotFound,
            service: service.into(),
            backtrace: Backtrace::capture(),
        }
    }

    /// The backend failed while operating on `service`.
    pub(crate) fn backend(service: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Backend {
                message: message.into(),
                source: None,
            },
            service: service.into(),
            backtrace: Backtrace::capture(),
        }
    }

    /// Like [`backend`](Self::backend), keeping the upstream error as the cause.
    pub(crate) fn backend_with_source(
        service: impl Into<String>,
        message: impl Into<String>,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: ErrorKind::Backend {
                message: message.into(),
                source: Some(Box::new(source)),
            },
            service: service.into(),
            backtrace: Backtrace::capture(),
        }
    }

    /// The name of the service the failed operation targeted.
    pub fn service(&self) -> &str {
        &self.service
    }

    /// Whether the spec was rejected before reaching the backend.
    pub fn is_invalid_spec(&self) -> bool {
        matches!(self.kind, ErrorKind::InvalidSpec { .. })
    }

    /// Whether the operation targeted a service the backend does not have.
    pub fn is_not_found(&self) -> bool {
        matches!(self.kind, ErrorKind::NotFound)
    }

    /// Whether the backend or its transport failed.
    pub fn is_backend(&self) -> bool {
        matches!(self.kind, ErrorKind::Backend { .. })
    }
}

impl std::fmt::Display for ProvisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            ErrorKind::InvalidSpec { message } => {
                write!(f, "invalid spec for service {:?}: {message}", self.service)?;
            }
            ErrorKind::NotFound => {
                write!(f, "no managed service named {:?}", self.service)?;
            }
            ErrorKind::Backend { message, source } => {
                write!(
                    f,
                    "provisioner backend failed for service {:?}: {message}",
                    self.service
                )?;
                if let Some(source) = source {
                    write!(f, "\ncaused by: {source}")?;
                }
            }
        }
        if self.backtrace.status() == BacktraceStatus::Captured {
            write!(f, "\n{}", self.backtrace)?;
        }
        Ok(())
    }
}

impl std::error::Error for ProvisionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match &self.kind {
            ErrorKind::Backend {
                source: Some(source),
                ..
            } => Some(source.as_ref()),
            _ => None,
        }
    }
}

/// A backend that reconciles [`ServiceSpec`]s against a container engine.
///
/// The single seam between the control plane and any engine; implementations
/// (Docker via a scoped socket-proxy, later Podman and Kubernetes) live in
/// sibling modules and never leak their engine types through these signatures.
/// Futures are declared `impl Future + Send` so every backend works under a
/// work-stealing runtime; consumers take `&impl Provisioner` and stay generic.
///
/// Semantics every backend must uphold, so the reconciler can re-drive the full
/// desired state at any time:
///
/// - `ensure` is idempotent: absent services are created and started, a changed
///   spec replaces the running service, and an unchanged spec is a no-op.
/// - `start`/`stop` are idempotent in their direction: starting a running
///   service or stopping a stopped one succeeds without effect. Both fail with
///   a not-found error when the service is absent.
/// - `remove` of an absent service is a no-op success.
/// - `inspect` reports absence as [`ServiceState::Absent`], not as an error.
pub trait Provisioner: Send + Sync {
    /// Converges the service to `spec`: create, replace, or leave as is.
    ///
    /// # Errors
    /// Fails when the spec is invalid ([`ServiceSpec::validate`]) or the
    /// backend cannot reach the desired state.
    fn ensure(
        &self,
        spec: &ServiceSpec,
    ) -> impl Future<Output = Result<ServiceHandle, ProvisionError>> + Send;

    /// Starts the named service if it is not already running.
    ///
    /// # Errors
    /// Fails when the service is absent or the backend cannot start it.
    fn start(&self, name: &str) -> impl Future<Output = Result<(), ProvisionError>> + Send;

    /// Stops the named service if it is running.
    ///
    /// # Errors
    /// Fails when the service is absent or the backend cannot stop it.
    fn stop(&self, name: &str) -> impl Future<Output = Result<(), ProvisionError>> + Send;

    /// Removes the named service entirely; a no-op if it is already absent.
    ///
    /// # Errors
    /// Fails only when the backend cannot remove an existing service.
    fn remove(&self, name: &str) -> impl Future<Output = Result<(), ProvisionError>> + Send;

    /// Reports the service's observed state; [`ServiceState::Absent`] if none.
    ///
    /// # Errors
    /// Fails only when the backend cannot be queried.
    fn inspect(
        &self,
        name: &str,
    ) -> impl Future<Output = Result<ServiceState, ProvisionError>> + Send;

    /// Returns the service's recent log output.
    ///
    /// A bounded tail for surfacing in the admin console; live streaming is a
    /// later extension of the seam.
    ///
    /// # Errors
    /// Fails when the service is absent or the backend cannot read its logs.
    fn logs(&self, name: &str) -> impl Future<Output = Result<String, ProvisionError>> + Send;
}

/// A cloneable, non-generic handle over the deployment's configured backend.
///
/// [`crate::state::AppState`] cannot be generic over a `Provisioner`, and the
/// trait's RPITIT futures rule out `dyn`, so this is the concrete dispatch enum
/// the trait docs anticipate: it implements [`Provisioner`] by forwarding to the
/// active backend, and consumers written as `&impl Provisioner` accept it
/// unchanged. `Clone` follows the `Arc<Inner>` service pattern (M-SERVICES-CLONE).
#[derive(Debug, Clone)]
pub enum ProvisionerHandle {
    /// The in-memory bookkeeping backend: full trait semantics, no real engine.
    ///
    /// The boot-time placeholder until the Docker backend lands; it converges
    /// desired state coherently but launches no actual containers.
    // TODO(#27): add a `Docker(DockerProvisioner)` variant (behind the scoped
    // socket-proxy) and select the backend from deployment config in
    // `AppState::new`, leaving this variant as the test double.
    InMemory(std::sync::Arc<memory::InMemoryProvisioner>),
}

impl ProvisionerHandle {
    /// Creates a handle over a fresh in-memory backend.
    pub fn in_memory() -> Self {
        Self::InMemory(std::sync::Arc::new(memory::InMemoryProvisioner::new()))
    }
}

// Pure delegation: the handle adds dispatch, never behavior. `async fn` satisfies
// the trait's `impl Future + Send` signatures since every backend future is Send.
impl Provisioner for ProvisionerHandle {
    async fn ensure(&self, spec: &ServiceSpec) -> Result<ServiceHandle, ProvisionError> {
        match self {
            Self::InMemory(backend) => backend.ensure(spec).await,
        }
    }

    async fn start(&self, name: &str) -> Result<(), ProvisionError> {
        match self {
            Self::InMemory(backend) => backend.start(name).await,
        }
    }

    async fn stop(&self, name: &str) -> Result<(), ProvisionError> {
        match self {
            Self::InMemory(backend) => backend.stop(name).await,
        }
    }

    async fn remove(&self, name: &str) -> Result<(), ProvisionError> {
        match self {
            Self::InMemory(backend) => backend.remove(name).await,
        }
    }

    async fn inspect(&self, name: &str) -> Result<ServiceState, ProvisionError> {
        match self {
            Self::InMemory(backend) => backend.inspect(name).await,
        }
    }

    async fn logs(&self, name: &str) -> Result<String, ProvisionError> {
        match self {
            Self::InMemory(backend) => backend.logs(name).await,
        }
    }
}

/// The in-memory provisioner: full trait semantics over a map, no real engine.
///
/// It serves two duties. In unit tests it is the fake the seam and the
/// reconciler are driven against. In the running binary it is the placeholder
/// backend behind [`ProvisionerHandle`] until the Docker backend (#27) lands:
/// desired state converges coherently, but no actual container runs. The
/// test-only observation and control helpers stay behind `#[cfg(test)]`.
pub mod memory {
    use std::collections::HashMap;
    use std::future::{ready, Future};
    use std::sync::Mutex;

    use super::{ProvisionError, Provisioner, ServiceHandle, ServiceSpec, ServiceState};

    /// One fake-managed service: its spec, state, and observable history.
    #[derive(Debug)]
    struct FakeService {
        spec: ServiceSpec,
        state: ServiceState,
        /// Bumped each time `ensure` replaces the service, so tests can prove
        /// an unchanged spec really was a no-op.
        generation: u64,
        /// Lifecycle log lines, served by `logs`.
        logs: Vec<String>,
    }

    /// A [`Provisioner`] holding its whole world in a map.
    ///
    /// Implements the exact trait semantics (idempotent `ensure`, no-op
    /// `remove` when absent, `Absent` from `inspect`) so tests against it
    /// exercise the same contract a real backend must honor.
    #[derive(Debug, Default)]
    pub struct InMemoryProvisioner {
        services: Mutex<HashMap<String, FakeService>>,
    }

    impl InMemoryProvisioner {
        /// Creates an empty in-memory backend with no services.
        pub fn new() -> Self {
            Self::default()
        }

        /// Test control: forces a service into `state` (e.g. `Unhealthy`),
        /// simulating an engine-side transition no trait call produces.
        ///
        /// # Panics
        /// Panics when the service does not exist; that is a broken test.
        #[cfg(test)]
        pub(crate) fn set_state(&self, name: &str, state: ServiceState) {
            let mut services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            services
                .get_mut(name)
                .expect("set_state targets an existing service")
                .state = state;
        }

        /// Test observation: the service's replace count, `None` if absent.
        #[cfg(test)]
        pub(crate) fn generation(&self, name: &str) -> Option<u64> {
            let services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            services.get(name).map(|service| service.generation)
        }

        /// Synchronous core of [`Provisioner::ensure`].
        fn ensure_sync(&self, spec: &ServiceSpec) -> Result<ServiceHandle, ProvisionError> {
            spec.validate()?;
            let mut services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            let service = match services.get_mut(&spec.name) {
                // Unchanged spec: converged already, touch nothing.
                Some(existing) if existing.spec == *spec => existing,
                // Changed spec: replace the container and start the new one.
                Some(existing) => {
                    existing.spec = spec.clone();
                    existing.generation += 1;
                    existing.state = ServiceState::Running;
                    existing.logs.push("recreated".to_string());
                    existing.logs.push("started".to_string());
                    existing
                }
                // Absent: create and start.
                None => services.entry(spec.name.clone()).or_insert(FakeService {
                    spec: spec.clone(),
                    state: ServiceState::Running,
                    generation: 1,
                    logs: vec!["created".to_string(), "started".to_string()],
                }),
            };
            Ok(ServiceHandle {
                name: spec.name.clone(),
                id: format!("in-memory:{}:{}", spec.name, service.generation),
                state: service.state,
            })
        }

        /// Synchronous core of [`Provisioner::start`].
        fn start_sync(&self, name: &str) -> Result<(), ProvisionError> {
            let mut services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            let service = services
                .get_mut(name)
                .ok_or_else(|| ProvisionError::not_found(name))?;
            match service.state {
                // Already running (healthy or not): nothing to start.
                ServiceState::Running | ServiceState::Unhealthy => {}
                ServiceState::Absent
                | ServiceState::Created
                | ServiceState::Stopped
                | ServiceState::Failed => {
                    service.state = ServiceState::Running;
                    service.logs.push("started".to_string());
                }
            }
            Ok(())
        }

        /// Synchronous core of [`Provisioner::stop`].
        fn stop_sync(&self, name: &str) -> Result<(), ProvisionError> {
            let mut services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            let service = services
                .get_mut(name)
                .ok_or_else(|| ProvisionError::not_found(name))?;
            match service.state {
                ServiceState::Running | ServiceState::Unhealthy => {
                    service.state = ServiceState::Stopped;
                    service.logs.push("stopped".to_string());
                }
                // Not running: nothing to stop.
                ServiceState::Absent
                | ServiceState::Created
                | ServiceState::Stopped
                | ServiceState::Failed => {}
            }
            Ok(())
        }

        /// Synchronous core of [`Provisioner::remove`].
        fn remove_sync(&self, name: &str) -> Result<(), ProvisionError> {
            let mut services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            services.remove(name);
            Ok(())
        }

        /// Synchronous core of [`Provisioner::inspect`].
        fn inspect_sync(&self, name: &str) -> Result<ServiceState, ProvisionError> {
            let services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            Ok(services
                .get(name)
                .map_or(ServiceState::Absent, |service| service.state))
        }

        /// Synchronous core of [`Provisioner::logs`].
        fn logs_sync(&self, name: &str) -> Result<String, ProvisionError> {
            let services = self
                .services
                .lock()
                .expect("fake provisioner mutex poisoned");
            services
                .get(name)
                .map(|service| service.logs.join("\n"))
                .ok_or_else(|| ProvisionError::not_found(name))
        }
    }

    // The logic is synchronous (a mutex over a map), so each trait method wraps
    // its inherent core in an already-ready future.
    impl Provisioner for InMemoryProvisioner {
        fn ensure(
            &self,
            spec: &ServiceSpec,
        ) -> impl Future<Output = Result<ServiceHandle, ProvisionError>> + Send {
            ready(self.ensure_sync(spec))
        }

        fn start(&self, name: &str) -> impl Future<Output = Result<(), ProvisionError>> + Send {
            ready(self.start_sync(name))
        }

        fn stop(&self, name: &str) -> impl Future<Output = Result<(), ProvisionError>> + Send {
            ready(self.stop_sync(name))
        }

        fn remove(&self, name: &str) -> impl Future<Output = Result<(), ProvisionError>> + Send {
            ready(self.remove_sync(name))
        }

        fn inspect(
            &self,
            name: &str,
        ) -> impl Future<Output = Result<ServiceState, ProvisionError>> + Send {
            ready(self.inspect_sync(name))
        }

        fn logs(&self, name: &str) -> impl Future<Output = Result<String, ProvisionError>> + Send {
            ready(self.logs_sync(name))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::memory::InMemoryProvisioner;
    use super::*;

    /// A minimal valid spec: pinned image, one env var, defaults elsewhere.
    fn forgejo_spec() -> ServiceSpec {
        let mut spec = ServiceSpec::new("forgejo", "codeberg.org/forgejo/forgejo:13.0.1");
        spec.env.insert(
            "FORGEJO__server__DOMAIN".to_string(),
            EnvValue::plain("git.local"),
        );
        spec
    }

    // --- Env values ----------------------------------------------------------

    #[test]
    fn env_value_debug_redacts_secrets() {
        // Mirrors the crypto::Crypto debug-redaction test: a secret value must
        // never reach a log through Debug, while a plain value stays readable.
        let key = "552d3454-d0d5-445d-ab9f-ef2ae3a8896a";
        let secret = EnvValue::secret(key);
        assert_eq!(format!("{secret:?}"), "<redacted>");
        assert_eq!(secret.expose(), key);
        assert!(secret.is_secret());

        let plain = EnvValue::plain("git.local");
        assert_eq!(format!("{plain:?}"), "\"git.local\"");
        assert_eq!(plain.expose(), "git.local");
        assert!(!plain.is_secret());
    }

    #[test]
    fn spec_debug_redacts_secret_env_values() {
        let key = "generated-admin-token-2b7f04c1";
        let mut spec = forgejo_spec();
        spec.env.insert(
            "FORGEJO__security__SECRET_KEY".to_string(),
            EnvValue::secret(key),
        );

        let rendered = format!("{spec:?}");
        // The key name and plain values stay visible for debugging; the secret
        // material itself never appears.
        assert!(rendered.contains("FORGEJO__security__SECRET_KEY"));
        assert!(rendered.contains("git.local"));
        assert!(!rendered.contains(key));
        assert!(rendered.contains("<redacted>"));
    }

    // --- Spec validation ----------------------------------------------------

    #[test]
    fn validate_accepts_a_pinned_tag_and_a_digest() {
        assert!(forgejo_spec().validate().is_ok());
        let digest = ServiceSpec::new(
            "registry",
            "registry.example.com/distribution@sha256:0123456789abcdef",
        );
        assert!(digest.validate().is_ok());
    }

    #[test]
    fn validate_rejects_unpinned_images() {
        // No tag at all, a floating `latest` tag, and an empty digest all fail;
        // a registry port must not be mistaken for a tag.
        for image in [
            "forgejo",
            "forgejo:latest",
            "forgejo@",
            "registry.example.com:5000/forgejo",
        ] {
            let error = ServiceSpec::new("forgejo", image)
                .validate()
                .expect_err("unpinned image accepted");
            assert!(error.is_invalid_spec(), "image {image:?} gave {error}");
        }
    }

    #[test]
    fn validate_rejects_bad_service_names() {
        for name in ["", "-leading-dash", "has space", "has/slash"] {
            let error = ServiceSpec::new(name, "forgejo:13.0.1")
                .validate()
                .expect_err("bad name accepted");
            assert!(error.is_invalid_spec(), "name {name:?} gave {error}");
            assert_eq!(error.service(), name);
        }
    }

    #[test]
    fn validate_rejects_relative_volume_paths() {
        let mut relative_host = forgejo_spec();
        relative_host.volumes.push(VolumeBind {
            host_source: "data/forgejo".to_string(),
            container_path: "/data".to_string(),
            read_only: false,
        });
        assert!(relative_host
            .validate()
            .expect_err("relative host")
            .is_invalid_spec());

        let mut relative_container = forgejo_spec();
        relative_container.volumes.push(VolumeBind {
            host_source: "/srv/elysium/forgejo".to_string(),
            container_path: "data".to_string(),
            read_only: false,
        });
        assert!(relative_container
            .validate()
            .expect_err("relative container path")
            .is_invalid_spec());
    }

    #[test]
    fn validate_rejects_zero_ports_and_empty_health_commands() {
        let mut zero_port = forgejo_spec();
        zero_port.ports.push(PortBind {
            host_port: 0,
            container_port: 3000,
            protocol: PortProtocol::Tcp,
        });
        assert!(zero_port
            .validate()
            .expect_err("zero port")
            .is_invalid_spec());

        let mut empty_probe = forgejo_spec();
        empty_probe.health_check = Some(HealthCheck {
            command: Vec::new(),
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(5),
            retries: 3,
        });
        assert!(empty_probe
            .validate()
            .expect_err("empty health command")
            .is_invalid_spec());
    }

    // --- Fake provisioner: ensure -------------------------------------------

    #[tokio::test]
    async fn ensure_creates_and_runs_the_service() {
        let provisioner = InMemoryProvisioner::new();
        let handle = provisioner.ensure(&forgejo_spec()).await.expect("ensure");

        assert_eq!(handle.name, "forgejo");
        assert_eq!(handle.state, ServiceState::Running);
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Running
        );
        assert_eq!(provisioner.generation("forgejo"), Some(1));
    }

    #[tokio::test]
    async fn ensure_is_idempotent_for_an_unchanged_spec() {
        let provisioner = InMemoryProvisioner::new();
        let first = provisioner
            .ensure(&forgejo_spec())
            .await
            .expect("first ensure");
        let second = provisioner
            .ensure(&forgejo_spec())
            .await
            .expect("second ensure");

        // The second call was a no-op: same handle, no replacement, no new logs.
        assert_eq!(first, second);
        assert_eq!(provisioner.generation("forgejo"), Some(1));
        assert_eq!(
            provisioner.logs("forgejo").await.expect("logs"),
            "created\nstarted"
        );
    }

    #[tokio::test]
    async fn ensure_replaces_the_service_when_the_spec_changes() {
        let provisioner = InMemoryProvisioner::new();
        provisioner
            .ensure(&forgejo_spec())
            .await
            .expect("initial ensure");
        provisioner.stop("forgejo").await.expect("stop");

        let mut upgraded = forgejo_spec();
        upgraded.image = "codeberg.org/forgejo/forgejo:13.0.2".to_string();
        let handle = provisioner.ensure(&upgraded).await.expect("upgrade ensure");

        // The changed spec replaced the container and brought it back up.
        assert_eq!(handle.state, ServiceState::Running);
        assert_eq!(provisioner.generation("forgejo"), Some(2));
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Running
        );
    }

    #[tokio::test]
    async fn ensure_rejects_an_invalid_spec_without_registering_it() {
        let provisioner = InMemoryProvisioner::new();
        let error = provisioner
            .ensure(&ServiceSpec::new("forgejo", "forgejo:latest"))
            .await
            .expect_err("floating tag accepted");

        assert!(error.is_invalid_spec());
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Absent
        );
    }

    // --- Fake provisioner: lifecycle transitions ----------------------------

    #[tokio::test]
    async fn stop_and_start_transition_between_running_and_stopped() {
        let provisioner = InMemoryProvisioner::new();
        provisioner.ensure(&forgejo_spec()).await.expect("ensure");

        provisioner.stop("forgejo").await.expect("stop");
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Stopped
        );
        // Stopping a stopped service is a no-op success.
        provisioner.stop("forgejo").await.expect("second stop");
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Stopped
        );

        provisioner.start("forgejo").await.expect("start");
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Running
        );
        // Starting a running service is a no-op success.
        provisioner.start("forgejo").await.expect("second start");
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Running
        );
    }

    #[tokio::test]
    async fn an_unhealthy_service_can_be_stopped() {
        let provisioner = InMemoryProvisioner::new();
        provisioner.ensure(&forgejo_spec()).await.expect("ensure");
        provisioner.set_state("forgejo", ServiceState::Unhealthy);

        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Unhealthy
        );
        provisioner.stop("forgejo").await.expect("stop");
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Stopped
        );
    }

    #[tokio::test]
    async fn remove_deletes_the_service_and_is_a_noop_when_absent() {
        let provisioner = InMemoryProvisioner::new();
        provisioner.ensure(&forgejo_spec()).await.expect("ensure");

        provisioner.remove("forgejo").await.expect("remove");
        assert_eq!(
            provisioner.inspect("forgejo").await.expect("inspect"),
            ServiceState::Absent
        );
        // Removing an already-absent service succeeds without effect.
        provisioner.remove("forgejo").await.expect("second remove");
        provisioner
            .remove("never-existed")
            .await
            .expect("remove of unknown");
    }

    #[tokio::test]
    async fn operations_on_an_absent_service_fail_with_not_found() {
        let provisioner = InMemoryProvisioner::new();

        for error in [
            provisioner.start("ghost").await.expect_err("start"),
            provisioner.stop("ghost").await.expect_err("stop"),
            provisioner.logs("ghost").await.expect_err("logs"),
        ] {
            assert!(error.is_not_found());
            assert_eq!(error.service(), "ghost");
        }
        // Inspect is the exception: absence is a state, not an error.
        assert_eq!(
            provisioner.inspect("ghost").await.expect("inspect"),
            ServiceState::Absent
        );
    }

    #[tokio::test]
    async fn logs_record_the_lifecycle_in_order() {
        let provisioner = InMemoryProvisioner::new();
        provisioner.ensure(&forgejo_spec()).await.expect("ensure");
        provisioner.stop("forgejo").await.expect("stop");
        provisioner.start("forgejo").await.expect("start");

        assert_eq!(
            provisioner.logs("forgejo").await.expect("logs"),
            "created\nstarted\nstopped\nstarted"
        );
    }

    // --- Error surface ------------------------------------------------------

    #[test]
    fn errors_render_their_context_and_expose_their_kind() {
        let not_found = ProvisionError::not_found("forgejo");
        assert!(not_found.to_string().contains("forgejo"));
        assert!(not_found.is_not_found());
        assert!(!not_found.is_backend());
        assert!(std::error::Error::source(&not_found).is_none());

        let backend = ProvisionError::backend_with_source(
            "forgejo",
            "engine unreachable",
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "socket refused"),
        );
        assert!(backend.is_backend());
        let rendered = backend.to_string();
        assert!(rendered.contains("engine unreachable"));
        assert!(rendered.contains("socket refused"));
        assert!(std::error::Error::source(&backend).is_some());
    }
}
