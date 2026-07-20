//! The desired-state reconciler: converges managed integrations to containers.
//!
//! Managed integrations are desired state in Postgres (`docs/integrations.md`);
//! this module brings actual engine state to it. [`reconcile_all`] runs on boot,
//! so a host reboot or image upgrade never loses a service, and
//! [`reconcile_integration`] runs after every management-API change. Both are
//! idempotent: every [`Provisioner`] operation converges rather than mutates, so
//! re-driving the full desired state is always safe.
//!
//! For each managed integration the reconciler builds a [`ServiceSpec`] (the
//! kind's blessed default image, volumes resolved from the storage-role bindings
//! through their Drives, and the env block rendered by the per-kind templates in
//! [`crate::orchestration::config`], service secret included) and walks the
//! row's `status` through `PROVISIONING -> RUNNING` (or `FAILED`); an explicit
//! stop converges to `STOPPED`, and a spec that cannot be built yet (a storage
//! role not bound) parks at `PENDING` with a human-readable `status_detail`. A
//! spec that can never be built as stored (a malformed config, an unopenable
//! secret) parks at `FAILED` with the reason. The secret is unsealed only here,
//! at injection time, and enters the spec as a redacted
//! [`crate::orchestration::provisioner::EnvValue`], so no reconcile log or
//! debug rendering ever carries it; because the secret is part of the spec, a
//! rotated credential changes the spec and the next converge re-injects it.
//!
//! One integration is one container: a project-scoped integration therefore gets
//! its own isolated container per project (`docs/projects.md` Runtime), named
//! `elysium-<kind>-<id>` on the shared control plane.

use std::collections::HashMap;

use sqlx::PgConnection;
use uuid::Uuid;

use crate::crypto::Crypto;
use crate::orchestration::config::{self, SecretPlan, ServiceSecret};
use crate::orchestration::integrations::{
    storage_roles, BindingRow, DesiredState, IntegrationKind, IntegrationMode, IntegrationRow,
    ReconcileStatus,
};
use crate::orchestration::provisioner::{
    PortBind, PortProtocol, Provisioner, ServiceSpec, ServiceState, VolumeBind,
};
use crate::orchestration::routing::{Route, RouteTable, MANAGED_UPSTREAM_HOST};
use crate::storage::{DriveConfig, DriveRow};

/// The blessed default image for each managed kind, pinned per the repo rule.
///
/// These are the opinionated defaults from `docs/integrations.md`, overridable
/// later through per-kind config once the per-service milestones land. Pinned
/// tags, never `latest`: [`ServiceSpec::validate`] rejects a floating tag, and a
/// pinned tag is what makes "is the running container this spec?" answerable.
/// The Astraval and runner images are JalapenoLabs artifacts not yet published;
/// their references are the intended coordinates and flow through the same seam.
pub fn default_image(kind: IntegrationKind) -> &'static str {
    match kind {
        // Pinned by tag *and* digest: the tag names the release, the digest makes
        // the pull byte-exact and immune to a re-tag. Forgejo 13.0.1 multi-arch
        // index digest, from codeberg.org/forgejo/-/packages/container/forgejo/13.0.1.
        IntegrationKind::Git => {
            "codeberg.org/forgejo/forgejo:13.0.1@sha256:\
             b34718b23370d19265f9e13ff28651f3258408360b4a13811184e45feff49456"
        }
        IntegrationKind::Mail => "stalwartlabs/stalwart:0.13.1",
        IntegrationKind::Registry => "registry:3.0.0",
        IntegrationKind::Perforce => "ghcr.io/jalapenolabs/astraval:0.1.0",
        IntegrationKind::Runner => "ghcr.io/jalapenolabs/elysium-runner:0.1.0",
    }
}

/// The deployment-unique service (container) name of a managed integration.
///
/// Keyed on the integration id, so every integration, and therefore every
/// project instance of a kind, maps to exactly one container, and reconcile
/// after a restart re-converges the same container instead of duplicating it.
pub fn service_name(integration: &IntegrationRow) -> String {
    format!("elysium-{}-{}", integration.kind.slug(), integration.id)
}

/// The container-side HTTP port a managed kind serves its embeddable web UI on.
///
/// `None` for kinds with no HTTP surface to embed in v1 (mail, perforce, runner);
/// their reachability is transport auth (#29 SSH/TCP) or the action-center, not an
/// iframe. The returned port is matched against the spec's published ports in
/// [`http_route_for`] to find the host port the proxy dials; a kind that has a
/// surface here but no matching published port simply gets no route yet (the
/// per-service milestone that publishes the port lights routing up automatically).
pub fn http_surface_port(kind: IntegrationKind) -> Option<u16> {
    match kind {
        // Forgejo's web UI (and its git-over-HTTP and websocket-backed live views).
        IntegrationKind::Git => Some(3000),
        // The registry's HTTP API and (with a UI image) its web console.
        IntegrationKind::Registry => Some(5000),
        IntegrationKind::Mail | IntegrationKind::Perforce | IntegrationKind::Runner => None,
    }
}

/// Derives the HTTP [`Route`] for an integration, or `None` when it has no surface.
///
/// A route exists only when the kind advertises an HTTP surface
/// ([`http_surface_port`]) *and* the planned spec publishes that container port to
/// a host port: the proxy reaches the container through
/// `host.docker.internal:{host_port}` (see [`crate::orchestration::routing`]). Pure
/// over the integration and its spec, so it is unit-tested without a database.
pub fn http_route_for(integration: &IntegrationRow, spec: &ServiceSpec) -> Option<Route> {
    let container_port = http_surface_port(integration.kind)?;
    let published = spec.ports.iter().find(|port| {
        port.container_port == container_port && matches!(port.protocol, PortProtocol::Tcp)
    })?;
    Some(Route::new(
        integration.id,
        MANAGED_UPSTREAM_HOST,
        published.host_port,
    ))
}

/// Whether a kind's milestone publishes its HTTP surface to a host port yet.
///
/// Publishing a surface port is what lights up a kind's proxy route (#29), and it
/// is opt-in per per-service milestone: git (this milestone) publishes, so its web
/// UI is reachable and its OAuth callback resolves; the registry's port lights up
/// with M5, and mail/perforce/runner have no web surface. Kept distinct from
/// [`http_surface_port`] so a kind can declare a surface before its milestone
/// chooses to expose it.
fn publishes_http_surface(kind: IntegrationKind) -> bool {
    matches!(kind, IntegrationKind::Git)
}

/// Base of the host-port band managed HTTP surfaces publish on.
///
/// Chosen above the common well-known/service ports and below the Linux ephemeral
/// range (`ip_local_port_range` defaults to 32768+), so an allocated publish port
/// never collides with the kernel's own outbound port allocation. Kept in sync
/// with the `host_port` CHECK in migration `0030`.
const MANAGED_HOST_PORT_BASE: u16 = 20000;

/// Width of the managed host-port band (ports 20000..=31999).
///
/// The band's inclusive upper bound is `MANAGED_HOST_PORT_BASE + MANAGED_HOST_PORT_SPAN - 1`.
/// Kept in sync with the `host_port` CHECK in migration `0030`.
const MANAGED_HOST_PORT_SPAN: u16 = 12000;

/// How many times [`ensure_host_port`] retries a lost allocation race.
///
/// Each retry follows a partial-unique-index rejection: a concurrent reconcile
/// claimed the same lowest-free port first, so we recompute against the now-larger
/// taken set and take the next gap. The band is 12000 wide, so realistic
/// contention resolves in one or two attempts; this bound only guards a
/// pathological storm and never trips in practice.
const HOST_PORT_ALLOCATION_ATTEMPTS: u32 = 16;

/// Allocates, or returns the existing, published host port for an integration.
///
/// Returns the row's `host_port` unchanged when one is already set, so the port is
/// stable across restarts and a re-reconcile never churns the container. Otherwise
/// it claims the lowest free port in the managed band (starting at
/// [`MANAGED_HOST_PORT_BASE`]) in one atomic statement and returns it. Deleting an
/// integration frees its port automatically (the column travels with the row), so
/// a later allocation reuses the lowest freed gap.
///
/// # Concurrency
/// The claim is a single `UPDATE ... FROM (lowest-free SELECT) WHERE host_port IS
/// NULL RETURNING`, holding no lock across the statement. Two reconciles racing for
/// *different* integrations can compute the same lowest-free port; the partial
/// UNIQUE index `integrations_host_port_key` admits exactly one, and the loser sees
/// a unique violation and retries against the now-larger taken set, taking the next
/// gap. Two reconciles racing for the *same* integration serialize on the
/// `host_port IS NULL` guard: the first sets it, the second matches no row,
/// re-reads, and returns the port the first stored. The read-committed snapshot the
/// lowest-free subquery sees may miss an in-flight peer's uncommitted claim, which
/// is exactly why the index, not the subquery, is the collision authority.
///
/// The retry relies on an **autocommit** connection: a unique violation aborts an
/// enclosing transaction, which would poison the next attempt. The reconciler
/// always drives this on a pooled connection (`main.rs` at boot,
/// `reconcile_and_respond` after a management-API change), never inside an open
/// transaction, so each attempt is its own implicit transaction.
///
/// # Errors
/// Returns [`sqlx::Error::RowNotFound`] when the band is fully allocated, so the
/// caller parks the integration `FAILED` rather than publishing a colliding port.
/// Surfaces the last unique violation if the retry bound is exhausted under
/// pathological contention, and propagates any genuine database error.
pub(crate) async fn ensure_host_port(
    conn: &mut PgConnection,
    integration_id: Uuid,
) -> sqlx::Result<u16> {
    // The band is inclusive on both ends; generate_series matches that.
    let band_end = MANAGED_HOST_PORT_BASE + MANAGED_HOST_PORT_SPAN - 1;
    let mut last_conflict: Option<sqlx::Error> = None;

    for _ in 0..HOST_PORT_ALLOCATION_ATTEMPTS {
        // Stability and the fast path: an already-allocated port is returned as-is,
        // never reallocated.
        if let Some(existing) = current_host_port(&mut *conn, integration_id).await? {
            return Ok(existing);
        }

        // Claim the lowest free port atomically. The `FROM (... LIMIT 1)` subquery
        // yields no row when the band is full, so the UPDATE then matches nothing;
        // the `host_port IS NULL` guard likewise makes a same-row double-claim a
        // no-op. The inner `WHERE host_port IS NOT NULL` keeps NULLs out of the
        // EXCEPT set (a NULL there would empty the whole candidate set).
        let claimed = sqlx::query_scalar::<_, i32>(
            "UPDATE integrations AS target
                SET host_port = free.port, updated_at = now()
               FROM (
                   SELECT candidate AS port
                     FROM (
                         SELECT gs AS candidate
                           FROM generate_series($2::int, $3::int) AS gs
                         EXCEPT
                         SELECT host_port
                           FROM integrations
                          WHERE host_port IS NOT NULL
                     ) AS candidates
                    ORDER BY candidate
                    LIMIT 1
               ) AS free
              WHERE target.id = $1
                AND target.host_port IS NULL
            RETURNING target.host_port",
        )
        .bind(integration_id)
        .bind(i32::from(MANAGED_HOST_PORT_BASE))
        .bind(i32::from(band_end))
        .fetch_optional(&mut *conn)
        .await;

        match claimed {
            Ok(Some(port)) => return Ok(port_from_db(port)),
            Ok(None) => {
                // The UPDATE matched no row: either a concurrent reconcile just set
                // our row (the fast path returns it on the next turn) or the band is
                // full. A re-read that is still NULL means genuine exhaustion.
                if current_host_port(&mut *conn, integration_id)
                    .await?
                    .is_none()
                {
                    return Err(sqlx::Error::RowNotFound);
                }
            }
            Err(error) if is_unique_violation(&error) => {
                // Another reconcile claimed our lowest-free pick first; retry, which
                // now sees a larger taken set and takes the next gap.
                last_conflict = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

    // Pathological contention only. Surface the last race so the caller parks
    // FAILED and the next reconcile pass retries from a quiescent state.
    Err(last_conflict.unwrap_or(sqlx::Error::RowNotFound))
}

/// Reads an integration's currently-allocated host port, `None` when unallocated.
async fn current_host_port(
    conn: &mut PgConnection,
    integration_id: Uuid,
) -> sqlx::Result<Option<u16>> {
    let stored: Option<i32> =
        sqlx::query_scalar::<_, Option<i32>>("SELECT host_port FROM integrations WHERE id = $1")
            .bind(integration_id)
            .fetch_optional(conn)
            .await?
            .flatten();
    Ok(stored.map(port_from_db))
}

/// Narrows a stored `host_port` to a `u16`; the band CHECK guarantees it fits.
///
/// # Panics
/// Panics if the column somehow holds a value outside the managed band, which the
/// `host_port` CHECK in migration `0030` makes impossible for a well-formed row.
fn port_from_db(value: i32) -> u16 {
    u16::try_from(value)
        .expect("the host_port CHECK constrains the column to the managed band [20000, 31999]")
}

/// Whether a sqlx error is a Postgres unique-constraint violation.
///
/// The signal a lost host-port allocation race raises against the partial UNIQUE
/// index, mirroring the same helper in [`crate::transport`] and [`crate::projects`].
fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
}

/// The deterministic OIDC client id a managed git integration registers under.
///
/// Derived from the integration id so a reconcile after restart finds and reuses
/// the same client instead of registering a duplicate; also the `--key` templated
/// into the Forgejo login source.
pub fn git_oidc_client_id(integration_id: Uuid) -> String {
    format!("forgejo-{integration_id}")
}

/// Whether a managed kind SSOs against Elysium's OIDC provider.
///
/// Only git today (Forgejo is registered as an OIDC client at provision time).
/// The registry and future kinds join here as their milestones wire SSO.
fn kind_uses_oidc(kind: IntegrationKind) -> bool {
    matches!(kind, IntegrationKind::Git)
}

/// Ensures the OIDC client backing a git integration and returns its credentials.
///
/// Registers (or, on re-reconcile, recovers) the deterministic `forgejo-<id>`
/// confidential first-party client whose single redirect URI is the Forgejo OAuth
/// callback under the reverse-proxy sub-path. The login source Forgejo registers
/// and the client Elysium stores therefore agree on both the id and the callback.
///
/// # Errors
/// Propagates a database or crypto failure from
/// [`crate::oidc::clients::ensure_integration_client`], or errors when the client
/// was somehow registered without a secret.
async fn ensure_oidc_client(
    conn: &mut PgConnection,
    crypto: &Crypto,
    app_origin: &str,
    integration: &IntegrationRow,
) -> eyre::Result<config::OidcClientCreds> {
    let client_id = git_oidc_client_id(integration.id);
    let base = app_origin.trim_end_matches('/');
    // Forgejo builds the callback as `<ROOT_URL>/user/oauth2/<source>/callback`;
    // ROOT_URL is `<origin>/services/<id>/`, so the redirect URI is this exact path.
    let redirect_uri = format!(
        "{base}/services/{}/user/oauth2/{}/callback",
        integration.id,
        config::OIDC_SOURCE_NAME,
    );
    let created = crate::oidc::clients::ensure_integration_client(
        conn,
        crypto,
        integration.id,
        &client_id,
        &[redirect_uri],
    )
    .await?;
    let secret = created.client_secret.ok_or_else(|| {
        eyre::eyre!("the integration OIDC client was registered without a secret")
    })?;
    Ok(config::OidcClientCreds {
        client_id: created.client_id,
        client_secret: ServiceSecret::new(secret),
    })
}

/// The outcome of planning a managed integration's [`ServiceSpec`].
#[derive(Debug)]
pub(crate) enum SpecPlan {
    /// Every storage role is resolved; the spec is ready to ensure.
    Ready(Box<ServiceSpec>),
    /// A storage role is not bound yet: not an error, the integration simply is
    /// not fully configured. Parks the row at `PENDING` with this detail.
    Waiting(String),
    /// The stored configuration is inconsistent (e.g. a bound Drive's config no
    /// longer parses). Parks the row at `FAILED` with this detail.
    Broken(String),
}

/// Plans the [`ServiceSpec`] for a managed integration from its bindings.
///
/// Pure: every input is loaded by the caller (the service secret through
/// [`config::ensure_service_secret`], the published `host_port` through
/// [`ensure_host_port`]), so spec construction is unit-tested without a database.
/// The env block comes from the per-kind template
/// ([`config::render_env`]); a config that does not fit the kind's shape parks
/// the plan as [`SpecPlan::Broken`]. Volumes come from the FS-backed bindings:
/// the Drive's root joined with the binding prefix (or the reserved
/// `.elysium/<kind>/<role>` area when the prefix is empty), mounted at the
/// role's declared container path. Object-store roles bound to a non-FS Drive
/// (e.g. registry blobs on S3) carry no host volume; translating those into
/// the service's provider-native settings (like the registry's S3 storage
/// driver) is per-kind env wiring owned by the per-service milestones on top
/// of this seam.
pub(crate) fn build_spec(
    integration: &IntegrationRow,
    bindings: &[BindingRow],
    drives: &HashMap<Uuid, DriveRow>,
    secret: Option<&ServiceSecret>,
    provisioning: &config::Provisioning<'_>,
    host_port: Option<u16>,
) -> SpecPlan {
    let mut spec = ServiceSpec::new(service_name(integration), default_image(integration.kind));

    // The per-kind template renders config_json plus the service secret and the
    // provisioning context (origin, OIDC client) into the env the kind's blessed
    // image expects. Secrets enter as redacted EnvValues, so the spec stays safe
    // to debug-format everywhere below.
    match config::render_env(integration, secret, provisioning) {
        Ok(env) => spec.env = env,
        Err(error) => return SpecPlan::Broken(error.to_string()),
    }

    // Publish the kind's HTTP surface on its allocated host port when its milestone
    // exposes it, so the reconciler's route registration ([`http_route_for`]) finds
    // the published port and the proxy can reach the container. The port is resolved
    // by the caller through [`ensure_host_port`] (persisted, unique per host); a
    // publishing kind reconciled without one simply carries no port and no route.
    if publishes_http_surface(integration.kind) {
        if let (Some(container_port), Some(host_port)) =
            (http_surface_port(integration.kind), host_port)
        {
            spec.ports.push(PortBind {
                host_port,
                container_port,
                protocol: PortProtocol::Tcp,
            });
        }
    }

    spec.labels
        .insert("io.elysium.managed".to_string(), "true".to_string());
    spec.labels.insert(
        "io.elysium.integration".to_string(),
        integration.id.to_string(),
    );
    spec.labels
        .insert("io.elysium.org".to_string(), integration.org_id.to_string());
    if let Some(project_id) = integration.project_id {
        spec.labels
            .insert("io.elysium.project".to_string(), project_id.to_string());
    }

    for role in storage_roles(integration.kind) {
        let Some(binding) = bindings.iter().find(|binding| binding.role == role.name) else {
            return SpecPlan::Waiting(format!(
                "waiting for storage role {:?} to be bound to a Drive",
                role.name
            ));
        };
        let Some(drive) = drives.get(&binding.drive_id) else {
            return SpecPlan::Broken(format!(
                "storage role {:?} is bound to a Drive that no longer exists",
                role.name
            ));
        };
        match drive.parse_config() {
            // Only a local-FS Drive resolves to a host path the engine can
            // mount. Other providers satisfy their capability contract through
            // provider-native service settings, not a bind mount (see above).
            Ok(DriveConfig::Fs(fs)) => {
                let sub = if binding.prefix.is_empty() {
                    format!(".elysium/{}/{}", integration.kind.slug(), role.name)
                } else {
                    binding.prefix.clone()
                };
                spec.volumes.push(VolumeBind {
                    host_source: format!("{}/{}", fs.root.trim_end_matches('/'), sub),
                    container_path: role.mount_path.to_string(),
                    read_only: false,
                });
            }
            Ok(_) => {}
            Err(error) => {
                return SpecPlan::Broken(format!(
                    "storage role {:?} is bound to a Drive with an invalid config: {error}",
                    role.name
                ));
            }
        }
    }

    SpecPlan::Ready(Box::new(spec))
}

/// Converges one service toward its desired state, reporting the outcome.
///
/// Pure with respect to the database: the caller persists the returned status and
/// detail. Desired **stopped** stops an existing container (leaving it
/// provisioned for a fast restart) and reports `STOPPED`. Desired **running**
/// ensures the planned spec, then starts the container when `ensure` found it
/// provisioned-but-stopped (an unchanged spec is a no-op by contract, so a
/// previously stopped service needs the explicit start), and reports `RUNNING`,
/// or `PENDING`/`FAILED` when the plan is waiting or the engine cannot converge.
pub(crate) async fn converge(
    provisioner: &impl Provisioner,
    service: &str,
    desired: DesiredState,
    plan: SpecPlan,
) -> (ReconcileStatus, String) {
    match desired {
        DesiredState::Stopped => match provisioner.inspect(service).await {
            Ok(ServiceState::Absent) => (
                ReconcileStatus::Stopped,
                "no container is provisioned".to_string(),
            ),
            Ok(_) => match provisioner.stop(service).await {
                Ok(()) => (ReconcileStatus::Stopped, String::new()),
                Err(error) => (ReconcileStatus::Failed, error.to_string()),
            },
            Err(error) => (ReconcileStatus::Failed, error.to_string()),
        },
        DesiredState::Running => match plan {
            SpecPlan::Waiting(detail) => (ReconcileStatus::Pending, detail),
            SpecPlan::Broken(detail) => (ReconcileStatus::Failed, detail),
            SpecPlan::Ready(spec) => match provisioner.ensure(&spec).await {
                Ok(handle) => match handle.state {
                    ServiceState::Running => (ReconcileStatus::Running, String::new()),
                    ServiceState::Unhealthy => (
                        ReconcileStatus::Failed,
                        "the container is running but failing its health check".to_string(),
                    ),
                    // Provisioned but not running (a previously stopped or
                    // failed container whose spec is unchanged): start it.
                    ServiceState::Created | ServiceState::Stopped | ServiceState::Failed => {
                        match provisioner.start(service).await {
                            Ok(()) => (ReconcileStatus::Running, String::new()),
                            Err(error) => (ReconcileStatus::Failed, error.to_string()),
                        }
                    }
                    ServiceState::Absent => (
                        ReconcileStatus::Failed,
                        "the engine reported the service absent after ensure".to_string(),
                    ),
                },
                Err(error) => (ReconcileStatus::Failed, error.to_string()),
            },
        },
    }
}

/// Reconciles every managed integration; the boot-time entry point.
///
/// Loads the full desired state and converges each row independently: one
/// integration's failure is recorded on its own row (`FAILED` + detail) and never
/// blocks the rest. Safe to run repeatedly; a re-run against a converged world is
/// a no-op per the [`Provisioner`] idempotency contract.
///
/// # Errors
/// Returns an error only when the desired state itself cannot be read; per-row
/// provisioning failures are persisted as status, not returned.
pub async fn reconcile_all(
    conn: &mut PgConnection,
    provisioner: &impl Provisioner,
    crypto: &Crypto,
    routes: &RouteTable,
    app_origin: &str,
) -> sqlx::Result<()> {
    let rows = sqlx::query_as::<_, IntegrationRow>(
        "SELECT * FROM integrations WHERE mode = $1 ORDER BY created_at ASC",
    )
    .bind(IntegrationMode::Managed.as_db())
    .fetch_all(&mut *conn)
    .await?;

    tracing::info!(count = rows.len(), "reconciling managed integrations");
    for row in rows {
        reconcile_row(conn, provisioner, crypto, routes, app_origin, &row).await?;
    }
    Ok(())
}

/// Reconciles one integration by id, after a management-API change.
///
/// A linked (or since-deleted) integration is a no-op: only managed rows have a
/// container to converge.
///
/// # Errors
/// Returns an error when the row or its bindings cannot be read, or a status
/// write fails.
pub async fn reconcile_integration(
    conn: &mut PgConnection,
    provisioner: &impl Provisioner,
    crypto: &Crypto,
    routes: &RouteTable,
    app_origin: &str,
    integration_id: Uuid,
) -> sqlx::Result<()> {
    let row = sqlx::query_as::<_, IntegrationRow>("SELECT * FROM integrations WHERE id = $1")
        .bind(integration_id)
        .fetch_optional(&mut *conn)
        .await?;
    match row {
        Some(row) if row.mode == IntegrationMode::Managed => {
            reconcile_row(conn, provisioner, crypto, routes, app_origin, &row).await
        }
        _ => Ok(()),
    }
}

/// The per-row body shared by [`reconcile_all`] and [`reconcile_integration`].
///
/// After converging the container, the row's HTTP route tracks the outcome: a
/// running integration with an HTTP surface and a published port registers its
/// route, and every other outcome (stopped, parked, failed, or a kind with no
/// surface) unregisters it. Because the boot reconcile walks every managed row,
/// this rebuilds the whole route table from integration state on startup.
async fn reconcile_row(
    conn: &mut PgConnection,
    provisioner: &impl Provisioner,
    crypto: &Crypto,
    routes: &RouteTable,
    app_origin: &str,
    row: &IntegrationRow,
) -> sqlx::Result<()> {
    let service = service_name(row);

    // Mark the walk so the console sees progress while a slow engine converges.
    set_status(&mut *conn, row.id, ReconcileStatus::Provisioning, "").await?;

    // Resolve the service secret at injection time only: unseal what is
    // stored, or mint and persist one where the kind consumes a secret and
    // none exists yet. An unusable stored secret parks the row FAILED.
    let secret = match config::ensure_service_secret(&mut *conn, crypto, row).await? {
        SecretPlan::NotNeeded => None,
        SecretPlan::Ready(secret) => Some(secret),
        SecretPlan::Broken(detail) => {
            return set_status(conn, row.id, ReconcileStatus::Failed, &detail).await;
        }
    };

    // Ensure the OIDC client a kind that SSOs against Elysium (git today) logs its
    // users in through, recovering its credentials for the service config. A
    // registration failure parks the row FAILED rather than crashing the reconcile.
    let oidc = if kind_uses_oidc(row.kind) {
        match ensure_oidc_client(&mut *conn, crypto, app_origin, row).await {
            Ok(creds) => Some(creds),
            Err(error) => {
                let detail = format!("failed to register the OIDC client: {error}");
                return set_status(conn, row.id, ReconcileStatus::Failed, &detail).await;
            }
        }
    } else {
        None
    };
    let provisioning = config::Provisioning {
        app_origin,
        oidc: oidc.as_ref(),
    };

    let bindings = crate::orchestration::integrations::bindings_for(&mut *conn, row.id).await?;
    let drive_ids: Vec<Uuid> = bindings.iter().map(|binding| binding.drive_id).collect();
    let drives: HashMap<Uuid, DriveRow> =
        sqlx::query_as::<_, DriveRow>("SELECT * FROM drives WHERE id = ANY($1)")
            .bind(&drive_ids)
            .fetch_all(&mut *conn)
            .await?
            .into_iter()
            .map(|drive| (drive.id, drive))
            .collect();

    // Allocate this integration's published host port (persisted, stable across
    // restarts, unique per host) when its kind exposes an HTTP surface. A full band
    // parks the row FAILED rather than publishing a colliding port; non-publishing
    // kinds carry no port.
    let host_port = if publishes_http_surface(row.kind) {
        match ensure_host_port(&mut *conn, row.id).await {
            Ok(port) => Some(port),
            Err(error) => {
                let detail = host_port_failure_detail(&error);
                return set_status(conn, row.id, ReconcileStatus::Failed, &detail).await;
            }
        }
    } else {
        None
    };

    let plan = build_spec(
        row,
        &bindings,
        &drives,
        secret.as_ref(),
        &provisioning,
        host_port,
    );
    // Derive the HTTP route from the planned spec before `converge` consumes it,
    // so the route can be registered or dropped based on the reconcile outcome.
    let http_route = match &plan {
        SpecPlan::Ready(spec) => http_route_for(row, spec),
        SpecPlan::Waiting(_) | SpecPlan::Broken(_) => None,
    };
    let (status, detail) = converge(provisioner, &service, row.desired, plan).await;

    // A route lives exactly as long as the integration serves HTTP: register it
    // once running with a published surface, and drop it on any other outcome.
    match (status, http_route) {
        (ReconcileStatus::Running, Some(route)) => routes.register(route),
        _ => routes.unregister(row.id),
    }

    tracing::info!(
        integration.id = %row.id,
        integration.kind = row.kind.slug(),
        service.name = %service,
        reconcile.status = ?status,
        reconcile.detail = %detail,
        "integration reconciled: {{service.name}} -> {{reconcile.status}}",
    );
    set_status(conn, row.id, status, &detail).await
}

/// Renders a host-port allocation failure into a console-facing status detail.
///
/// A full band ([`sqlx::Error::RowNotFound`] from [`ensure_host_port`]) names the
/// band so an operator sees the deployment is out of publish ports; any other error
/// is a database or contention failure the next reconcile pass retries.
fn host_port_failure_detail(error: &sqlx::Error) -> String {
    match error {
        sqlx::Error::RowNotFound => format!(
            "no free host port in the managed band [{}, {}]",
            MANAGED_HOST_PORT_BASE,
            MANAGED_HOST_PORT_BASE + MANAGED_HOST_PORT_SPAN - 1
        ),
        other => format!("could not allocate a host port: {other}"),
    }
}

/// Persists a reconcile outcome onto the integration row.
//
// TODO(follow-up): fan a status-change event out over SSE (crate::events) so the
// integrations console updates live instead of on refetch. Needs a realtime.proto
// event shape; tracked as the SSE-status-event follow-up to #28.
async fn set_status(
    conn: &mut PgConnection,
    integration_id: Uuid,
    status: ReconcileStatus,
    detail: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE integrations SET status = $1, status_detail = $2, updated_at = now()
          WHERE id = $3",
    )
    .bind(status.as_db())
    .bind(detail)
    .bind(integration_id)
    .execute(conn)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::integrations::{IntegrationVisibility, OwnerScope};
    use crate::orchestration::provisioner::memory::InMemoryProvisioner;
    use crate::storage::{DriveKind, DriveVisibility};
    use chrono::Utc;

    /// A fixed in-band host port for spec-planning tests that do not allocate.
    ///
    /// Allocation from the band is covered by the DB-gated [`ensure_host_port`]
    /// tests; the pure spec tests only need a concrete published port.
    const TEST_HOST_PORT: u16 = MANAGED_HOST_PORT_BASE;

    /// A managed integration row of `kind`, project-scoped, desired running.
    fn managed_row(kind: IntegrationKind) -> IntegrationRow {
        IntegrationRow {
            id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            project_id: Some(Uuid::new_v4()),
            kind,
            mode: IntegrationMode::Managed,
            owner_scope: OwnerScope::Project,
            owner_id: Uuid::new_v4(),
            name: "svc".to_string(),
            visibility: IntegrationVisibility::OrgWide,
            config: serde_json::json!({}),
            secret: None,
            desired: DesiredState::Running,
            status: ReconcileStatus::Pending,
            status_detail: String::new(),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// A DriveRow of `kind` with the given config, for spec-planning tests.
    fn drive(kind: DriveKind, config: serde_json::Value) -> DriveRow {
        DriveRow {
            id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            org_id: Uuid::new_v4(),
            name: "drive".to_string(),
            kind,
            config,
            secret: None,
            visibility: DriveVisibility::OrgWide,
            is_default: false,
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn binding(
        integration: &IntegrationRow,
        role: &str,
        drive: &DriveRow,
        prefix: &str,
    ) -> BindingRow {
        BindingRow {
            integration_id: integration.id,
            role: role.to_string(),
            drive_id: drive.id,
            prefix: prefix.to_string(),
        }
    }

    // --- Spec planning -------------------------------------------------------

    #[test]
    fn build_spec_resolves_fs_bindings_to_volumes() {
        let row = managed_row(IntegrationKind::Git);
        let fs = drive(
            DriveKind::Fs,
            serde_json::json!({ "root": "/srv/elysium/proj/" }),
        );
        let bindings = vec![binding(&row, "repos", &fs, "")];
        let drives = HashMap::from([(fs.id, fs)]);

        let SpecPlan::Ready(spec) = build_spec(
            &row,
            &bindings,
            &drives,
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        ) else {
            panic!("a fully-bound git integration plans a ready spec");
        };
        // The kind's blessed pinned image, a deployment-unique name, and the
        // Drive root joined with the reserved consumer area, mounted at the
        // role's container path.
        assert_eq!(spec.name, format!("elysium-git-{}", row.id));
        assert_eq!(spec.image, default_image(IntegrationKind::Git));
        assert_eq!(spec.volumes.len(), 1);
        assert_eq!(
            spec.volumes[0].host_source,
            "/srv/elysium/proj/.elysium/git/repos"
        );
        assert_eq!(spec.volumes[0].container_path, "/data");
        assert!(!spec.volumes[0].read_only);
        // The spec passes the provisioner's backend-independent validation.
        spec.validate().expect("planned specs are always valid");
        // Managed-by labels let an engine-side sweep identify Elysium containers.
        assert_eq!(spec.labels["io.elysium.managed"], "true");
        assert_eq!(spec.labels["io.elysium.integration"], row.id.to_string());
        assert_eq!(
            spec.labels["io.elysium.project"],
            row.project_id.expect("project scoped").to_string()
        );
    }

    #[test]
    fn build_spec_honors_an_explicit_binding_prefix() {
        let row = managed_row(IntegrationKind::Git);
        let fs = drive(DriveKind::Fs, serde_json::json!({ "root": "/srv/data" }));
        let bindings = vec![binding(&row, "repos", &fs, "forges/main")];
        let drives = HashMap::from([(fs.id, fs)]);

        let SpecPlan::Ready(spec) = build_spec(
            &row,
            &bindings,
            &drives,
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        ) else {
            panic!("ready");
        };
        assert_eq!(spec.volumes[0].host_source, "/srv/data/forges/main");
    }

    #[test]
    fn build_spec_waits_on_an_unbound_role() {
        let row = managed_row(IntegrationKind::Git);
        let SpecPlan::Waiting(detail) = build_spec(
            &row,
            &[],
            &HashMap::new(),
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        ) else {
            panic!("an unbound required role parks the plan as waiting");
        };
        assert!(detail.contains("repos"), "names the missing role: {detail}");
    }

    #[test]
    fn build_spec_gives_object_store_roles_on_s3_no_volume() {
        // Registry blobs on S3 is a legal binding, but S3 has no host path to
        // mount; the provider-native wiring is per-kind env (M2-06 +
        // per-service milestones), so the spec simply carries no volume.
        let row = managed_row(IntegrationKind::Registry);
        let s3 = drive(DriveKind::S3, serde_json::json!({ "bucket": "blobs" }));
        let bindings = vec![binding(&row, "blobs", &s3, "")];
        let drives = HashMap::from([(s3.id, s3)]);

        let SpecPlan::Ready(spec) = build_spec(
            &row,
            &bindings,
            &drives,
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        ) else {
            panic!("ready");
        };
        assert!(spec.volumes.is_empty());
        assert_eq!(spec.image, default_image(IntegrationKind::Registry));
    }

    #[test]
    fn build_spec_breaks_on_a_corrupt_drive_config() {
        let row = managed_row(IntegrationKind::Git);
        // An FS Drive whose stored config lost its root no longer parses.
        let bad = drive(DriveKind::Fs, serde_json::json!({ "root": "" }));
        let bindings = vec![binding(&row, "repos", &bad, "")];
        let drives = HashMap::from([(bad.id, bad)]);

        let SpecPlan::Broken(detail) = build_spec(
            &row,
            &bindings,
            &drives,
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        ) else {
            panic!("a corrupt Drive config breaks the plan");
        };
        assert!(detail.contains("repos"));
    }

    #[test]
    fn build_spec_breaks_on_a_vanished_drive() {
        let row = managed_row(IntegrationKind::Git);
        let fs = drive(DriveKind::Fs, serde_json::json!({ "root": "/srv" }));
        let bindings = vec![binding(&row, "repos", &fs, "")];
        // The drives map does not contain the bound Drive.
        let SpecPlan::Broken(detail) = build_spec(
            &row,
            &bindings,
            &HashMap::new(),
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        ) else {
            panic!("a vanished Drive breaks the plan");
        };
        assert!(detail.contains("no longer exists"));
    }

    #[test]
    fn build_spec_injects_the_rendered_env_and_secret() {
        let row = managed_row(IntegrationKind::Git);
        let secret = ServiceSecret::new("known-secret-value-77aa");

        let SpecPlan::Ready(spec) = ready_plan_with_secret(&row, Some(&secret)) else {
            panic!("ready");
        };
        // The per-kind template's env rides the spec: the base identity, the
        // kind's own entries, and the service secret tagged secret.
        assert_eq!(
            spec.env["ELYSIUM_INTEGRATION_ID"].expose(),
            row.id.to_string()
        );
        assert_eq!(spec.env["FORGEJO__security__INSTALL_LOCK"].expose(), "true");
        let key = &spec.env["FORGEJO__security__SECRET_KEY"];
        assert_eq!(key.expose(), "known-secret-value-77aa");
        assert!(key.is_secret());
    }

    #[test]
    fn build_spec_debug_output_never_contains_the_secret() {
        // The spec-building path's rendered debug output (what any log of a
        // plan or spec would carry) must redact the injected secret.
        let row = managed_row(IntegrationKind::Git);
        let secret = ServiceSecret::new("known-secret-value-3c1b");
        let plan = ready_plan_with_secret(&row, Some(&secret));

        let rendered = format!("{plan:?}");
        assert!(!rendered.contains("known-secret-value-3c1b"));
        assert!(rendered.contains("FORGEJO__security__SECRET_KEY"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn build_spec_breaks_on_a_malformed_config() {
        // A stored config that does not fit the kind's shape parks the plan
        // as Broken (the row goes FAILED with the reason), never a panic.
        let mut row = managed_row(IntegrationKind::Git);
        row.config = serde_json::json!({ "domain": 123 });

        let SpecPlan::Broken(detail) = ready_plan(&row) else {
            panic!("a malformed config breaks the plan");
        };
        assert!(detail.contains("malformed config_json"), "detail: {detail}");
    }

    #[tokio::test]
    async fn a_rotated_secret_reinjects_on_the_next_converge() {
        // Rotating the stored secret changes the rendered env, so the spec
        // differs and ensure replaces the container on the next reconcile.
        let provisioner = InMemoryProvisioner::new();
        let row = managed_row(IntegrationKind::Git);
        let service = service_name(&row);

        let first = ServiceSecret::new("initial-secret");
        converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan_with_secret(&row, Some(&first)),
        )
        .await;
        assert_eq!(provisioner.generation(&service), Some(1));

        // Same secret: converged already, nothing replaced.
        converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan_with_secret(&row, Some(&first)),
        )
        .await;
        assert_eq!(provisioner.generation(&service), Some(1));

        // Rotated secret: the container is replaced with the new injection.
        let rotated = ServiceSecret::new("rotated-secret");
        let (status, _) = converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan_with_secret(&row, Some(&rotated)),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Running);
        assert_eq!(provisioner.generation(&service), Some(2));
    }

    #[test]
    fn every_default_image_is_pinned() {
        // A floating default image would fail ServiceSpec::validate at reconcile
        // time; catch it at the table instead.
        for kind in IntegrationKind::ALL {
            let spec = ServiceSpec::new("probe", default_image(kind));
            spec.validate()
                .unwrap_or_else(|error| panic!("{kind:?} image is not pinned: {error}"));
        }
    }

    #[test]
    fn the_git_image_pins_both_a_tag_and_a_digest() {
        // The digest makes the pull byte-exact; the reference must carry both.
        let image = default_image(IntegrationKind::Git);
        assert!(image.contains("codeberg.org/forgejo/forgejo:13.0.1"));
        assert!(
            image.contains("@sha256:"),
            "the git image must pin a digest"
        );
        // The `\` line-continuation in the source must not have leaked whitespace
        // into the reference.
        assert!(!image.contains(' '), "no stray whitespace in the image ref");
    }

    // --- Surface publishing, host port, OIDC id -----------------------------

    #[test]
    fn git_spec_publishes_the_web_surface_and_registers_a_route() {
        // A fully-bound git integration publishes container port 3000 on its
        // allocated host port, and that published surface yields a proxy route.
        let row = managed_row(IntegrationKind::Git);
        let SpecPlan::Ready(spec) = ready_plan(&row) else {
            panic!("ready");
        };
        let published = spec
            .ports
            .iter()
            .find(|port| port.container_port == 3000)
            .expect("the Forgejo web port is published");
        assert_eq!(published.host_port, TEST_HOST_PORT);
        assert!(matches!(published.protocol, PortProtocol::Tcp));

        let route = http_route_for(&row, &spec).expect("a published surface routes");
        assert_eq!(route.upstream_port, TEST_HOST_PORT);

        // The templated HTTP_PORT matches the routed container surface, so Forgejo
        // listens where the proxy dials.
        assert_eq!(
            spec.env["FORGEJO__server__HTTP_PORT"].expose(),
            http_surface_port(IntegrationKind::Git)
                .expect("git has a surface")
                .to_string()
        );
    }

    #[test]
    fn only_a_surfaced_milestone_publishes_a_port() {
        // Git's milestone publishes; the registry's port stays unpublished until M5,
        // so its spec carries no port and gets no route yet.
        let registry = managed_row(IntegrationKind::Registry);
        let s3 = drive(DriveKind::S3, serde_json::json!({ "bucket": "blobs" }));
        let bindings = vec![binding(&registry, "blobs", &s3, "")];
        let drives = HashMap::from([(s3.id, s3)]);
        let SpecPlan::Ready(spec) = build_spec(
            &registry,
            &bindings,
            &drives,
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        ) else {
            panic!("ready");
        };
        assert!(spec.ports.is_empty(), "the registry does not publish yet");
        assert!(http_route_for(&registry, &spec).is_none());
    }

    #[test]
    fn a_publishing_kind_without_an_allocated_port_carries_none() {
        // build_spec only publishes when the caller resolved a host port; a git
        // integration planned without one carries no port and no route (the
        // reconciler always resolves one first, but the spec stays honest).
        let row = managed_row(IntegrationKind::Git);
        let fs = drive(DriveKind::Fs, serde_json::json!({ "root": "/srv/data" }));
        let bindings = vec![binding(&row, "repos", &fs, "")];
        let drives = HashMap::from([(fs.id, fs)]);
        let SpecPlan::Ready(spec) = build_spec(
            &row,
            &bindings,
            &drives,
            None,
            &config::Provisioning::default(),
            None,
        ) else {
            panic!("ready");
        };
        assert!(spec.ports.is_empty(), "no port without an allocation");
        assert!(http_route_for(&row, &spec).is_none());
    }

    #[test]
    fn the_managed_band_sits_below_the_ephemeral_range() {
        // The band the persisted allocation draws from, and the migration 0030
        // CHECK, agree on [20000, 31999], entirely below the Linux ephemeral floor
        // (32768) so a published port never fights the kernel's outbound allocation.
        assert_eq!(MANAGED_HOST_PORT_BASE, 20000);
        assert_eq!(MANAGED_HOST_PORT_SPAN, 12000);
        let band_end = MANAGED_HOST_PORT_BASE + MANAGED_HOST_PORT_SPAN - 1;
        assert_eq!(band_end, 31999);
        assert!(band_end < 32768, "the band stays below the ephemeral range");
    }

    #[test]
    fn git_oidc_client_id_is_deterministic_and_prefixed() {
        let id = Uuid::new_v4();
        assert_eq!(git_oidc_client_id(id), format!("forgejo-{id}"));
        assert_eq!(git_oidc_client_id(id), git_oidc_client_id(id));
    }

    // --- HTTP route derivation ----------------------------------------------

    #[test]
    fn http_surface_port_covers_every_kind() {
        // The web-surfaced kinds carry a port; the transport-only kinds do not.
        assert_eq!(http_surface_port(IntegrationKind::Git), Some(3000));
        assert_eq!(http_surface_port(IntegrationKind::Registry), Some(5000));
        assert_eq!(http_surface_port(IntegrationKind::Mail), None);
        assert_eq!(http_surface_port(IntegrationKind::Perforce), None);
        assert_eq!(http_surface_port(IntegrationKind::Runner), None);
    }

    #[test]
    fn http_route_for_maps_a_published_surface_to_the_host_gateway() {
        use crate::orchestration::provisioner::{PortBind, PortProtocol};

        // A git integration whose spec publishes the Forgejo web port (3000) on a
        // host port routes to host.docker.internal at that host port.
        let row = managed_row(IntegrationKind::Git);
        let mut spec = ServiceSpec::new(service_name(&row), default_image(IntegrationKind::Git));
        spec.ports.push(PortBind {
            host_port: 38080,
            container_port: 3000,
            protocol: PortProtocol::Tcp,
        });

        let route = http_route_for(&row, &spec).expect("a published surface yields a route");
        assert_eq!(route.integration_id, row.id);
        assert_eq!(route.upstream_host, MANAGED_UPSTREAM_HOST);
        assert_eq!(route.upstream_port, 38080);
    }

    #[test]
    fn http_route_for_needs_the_surface_port_published() {
        use crate::orchestration::provisioner::{PortBind, PortProtocol};

        // A git spec that publishes some other port, but not the web surface, has
        // no HTTP route: routing lights up only once the surface port is published.
        let row = managed_row(IntegrationKind::Git);
        let mut spec = ServiceSpec::new(service_name(&row), default_image(IntegrationKind::Git));
        spec.ports.push(PortBind {
            host_port: 2222,
            container_port: 22,
            protocol: PortProtocol::Tcp,
        });
        assert!(http_route_for(&row, &spec).is_none());

        // With no published ports at all (today's build_spec), still no route.
        let bare = ServiceSpec::new(service_name(&row), default_image(IntegrationKind::Git));
        assert!(http_route_for(&row, &bare).is_none());
    }

    #[test]
    fn http_route_for_is_none_for_a_kind_without_a_surface() {
        use crate::orchestration::provisioner::{PortBind, PortProtocol};

        // Even if a mail spec published port 3000, mail advertises no HTTP surface.
        let row = managed_row(IntegrationKind::Mail);
        let mut spec = ServiceSpec::new(service_name(&row), default_image(IntegrationKind::Mail));
        spec.ports.push(PortBind {
            host_port: 3000,
            container_port: 3000,
            protocol: PortProtocol::Tcp,
        });
        assert!(http_route_for(&row, &spec).is_none());
    }

    // --- Convergence against the fake provisioner ----------------------------

    /// Plans the ready spec for `row` bound to a valid FS drive.
    fn ready_plan(row: &IntegrationRow) -> SpecPlan {
        ready_plan_with_secret(row, None)
    }

    /// Like [`ready_plan`], with a service secret to inject.
    fn ready_plan_with_secret(row: &IntegrationRow, secret: Option<&ServiceSecret>) -> SpecPlan {
        let fs = drive(DriveKind::Fs, serde_json::json!({ "root": "/srv/data" }));
        let bindings = vec![binding(row, "repos", &fs, "")];
        let drives = HashMap::from([(fs.id, fs)]);
        build_spec(
            row,
            &bindings,
            &drives,
            secret,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        )
    }

    #[tokio::test]
    async fn converge_drives_a_ready_plan_to_running_idempotently() {
        let provisioner = InMemoryProvisioner::new();
        let row = managed_row(IntegrationKind::Git);
        let service = service_name(&row);

        let (status, detail) = converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan(&row),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Running);
        assert_eq!(detail, "");
        assert_eq!(provisioner.generation(&service), Some(1));

        // Re-running against an unchanged desired state is a no-op: still
        // RUNNING, and the container was not replaced (the restart-safety
        // acceptance: re-convergence never duplicates containers).
        let (status, _) = converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan(&row),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Running);
        assert_eq!(provisioner.generation(&service), Some(1));
    }

    #[tokio::test]
    async fn converge_parks_a_waiting_plan_as_pending() {
        let provisioner = InMemoryProvisioner::new();
        let row = managed_row(IntegrationKind::Git);
        let plan = build_spec(
            &row,
            &[],
            &HashMap::new(),
            None,
            &config::Provisioning::default(),
            Some(TEST_HOST_PORT),
        );

        let (status, detail) = converge(
            &provisioner,
            &service_name(&row),
            DesiredState::Running,
            plan,
        )
        .await;
        assert_eq!(status, ReconcileStatus::Pending);
        assert!(detail.contains("repos"));
        // Nothing was provisioned while the configuration is incomplete.
        assert_eq!(provisioner.generation(&service_name(&row)), None);
    }

    #[tokio::test]
    async fn converge_reports_a_broken_plan_as_failed() {
        let provisioner = InMemoryProvisioner::new();
        let row = managed_row(IntegrationKind::Git);
        let plan = SpecPlan::Broken(
            "storage role \"repos\" is bound to a Drive that no longer exists".to_string(),
        );

        let (status, detail) = converge(
            &provisioner,
            &service_name(&row),
            DesiredState::Running,
            plan,
        )
        .await;
        assert_eq!(status, ReconcileStatus::Failed);
        assert!(detail.contains("repos"));
    }

    #[tokio::test]
    async fn converge_stops_a_running_service_and_reports_stopped() {
        let provisioner = InMemoryProvisioner::new();
        let row = managed_row(IntegrationKind::Git);
        let service = service_name(&row);

        // Bring it up, then converge toward stopped.
        converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan(&row),
        )
        .await;
        let (status, detail) = converge(
            &provisioner,
            &service,
            DesiredState::Stopped,
            ready_plan(&row),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Stopped);
        assert_eq!(detail, "");
        assert_eq!(
            provisioner.inspect(&service).await.expect("inspect"),
            ServiceState::Stopped
        );

        // Stopping again stays stopped; starting again reuses the container.
        let (status, _) = converge(
            &provisioner,
            &service,
            DesiredState::Stopped,
            ready_plan(&row),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Stopped);
        let (status, _) = converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan(&row),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Running);
        assert_eq!(provisioner.generation(&service), Some(1));
    }

    #[tokio::test]
    async fn converge_reports_stopped_when_nothing_is_provisioned() {
        let provisioner = InMemoryProvisioner::new();
        let row = managed_row(IntegrationKind::Git);

        let (status, detail) = converge(
            &provisioner,
            &service_name(&row),
            DesiredState::Stopped,
            build_spec(
                &row,
                &[],
                &HashMap::new(),
                None,
                &config::Provisioning::default(),
                None,
            ),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Stopped);
        assert_eq!(detail, "no container is provisioned");
    }

    #[tokio::test]
    async fn converge_flags_an_unhealthy_container_as_failed() {
        let provisioner = InMemoryProvisioner::new();
        let row = managed_row(IntegrationKind::Git);
        let service = service_name(&row);

        converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan(&row),
        )
        .await;
        provisioner.set_state(&service, ServiceState::Unhealthy);

        let (status, detail) = converge(
            &provisioner,
            &service,
            DesiredState::Running,
            ready_plan(&row),
        )
        .await;
        assert_eq!(status, ReconcileStatus::Failed);
        assert!(detail.contains("health check"));
    }

    // --- End-to-end against postgres (ignored by default) --------------------
    //
    // Run with `cargo test -- --ignored` and a DATABASE_URL; everything happens
    // inside a rolled-back transaction, exactly like the storage matrices.

    use sqlx::PgConnection;

    /// A fixed 32-byte key for deterministic crypto in DB tests.
    fn test_crypto() -> Crypto {
        Crypto::new(&[3u8; 32]).expect("32-byte key is valid")
    }

    async fn connect_test_pool() -> sqlx::PgPool {
        use sqlx::postgres::PgPoolOptions;

        let database_url =
            std::env::var("DATABASE_URL").expect("DATABASE_URL must be set for this test");
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .connect(&database_url)
            .await
            .expect("could not connect to postgres");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations must apply cleanly");
        pool
    }

    /// Seeds an org, project, FS Drive, and a managed git integration bound to it.
    async fn seed_bound_git(conn: &mut PgConnection) -> IntegrationRow {
        let unique = Uuid::new_v4();
        let org = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ('Reconcile Org', $1) RETURNING id",
        )
        .bind(format!("rec-{unique}"))
        .fetch_one(&mut *conn)
        .await
        .expect("insert org");
        let project = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO projects (org_id, name, slug) VALUES ($1, 'P', $2) RETURNING id",
        )
        .bind(org)
        .bind(format!("p-{unique}"))
        .fetch_one(&mut *conn)
        .await
        .expect("insert project");
        let drive = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO drives (project_id, org_id, name, kind, config)
             VALUES ($1, $2, 'fs', $3, $4) RETURNING id",
        )
        .bind(project)
        .bind(org)
        .bind(DriveKind::Fs.as_db())
        .bind(serde_json::json!({ "root": "/srv/elysium/test" }))
        .fetch_one(&mut *conn)
        .await
        .expect("insert drive");

        let row = sqlx::query_as::<_, IntegrationRow>(
            "INSERT INTO integrations
                 (org_id, project_id, kind, mode, owner_scope, owner_id, name)
             VALUES ($1, $2, $3, $4, $5, $2, 'Forge') RETURNING *",
        )
        .bind(org)
        .bind(project)
        .bind(IntegrationKind::Git.as_db())
        .bind(IntegrationMode::Managed.as_db())
        .bind(OwnerScope::Project.as_db())
        .fetch_one(&mut *conn)
        .await
        .expect("insert integration");
        sqlx::query(
            "INSERT INTO integration_storage_bindings (integration_id, role, drive_id)
             VALUES ($1, 'repos', $2)",
        )
        .bind(row.id)
        .bind(drive)
        .execute(&mut *conn)
        .await
        .expect("insert binding");
        row
    }

    /// Counts the confidential, first-party OIDC clients registered for `id`.
    ///
    /// Under the deterministic `forgejo-<id>` client id, so a duplicate would show
    /// as a count above one.
    async fn confidential_client_count(conn: &mut PgConnection, id: Uuid) -> i64 {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM oidc_clients
              WHERE client_id = $1 AND client_secret IS NOT NULL
                AND is_first_party AND integration_id = $2",
        )
        .bind(git_oidc_client_id(id))
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("count oidc clients")
    }

    async fn status_of(conn: &mut PgConnection, id: Uuid) -> (ReconcileStatus, String) {
        let (status, detail) = sqlx::query_as::<_, (i16, String)>(
            "SELECT status, status_detail FROM integrations WHERE id = $1",
        )
        .bind(id)
        .fetch_one(conn)
        .await
        .expect("read status");
        (
            ReconcileStatus::from_db(status).expect("known status"),
            detail,
        )
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn reconcile_all_converges_and_persists_status_idempotently() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let provisioner = InMemoryProvisioner::new();
        let crypto = test_crypto();
        let routes = RouteTable::new();

        let app_origin = "https://elysium.test";
        let bound = seed_bound_git(&mut tx).await;
        assert_eq!(
            bound.status,
            ReconcileStatus::Pending,
            "fresh rows are pending"
        );

        // First reconcile brings the bound integration to RUNNING and persists it.
        reconcile_all(&mut tx, &provisioner, &crypto, &routes, app_origin)
            .await
            .expect("reconcile");
        let (status, detail) = status_of(&mut tx, bound.id).await;
        assert_eq!(status, ReconcileStatus::Running);
        assert_eq!(detail, "");
        let service = service_name(&bound);
        assert_eq!(provisioner.generation(&service), Some(1));

        // A git integration consumes a service secret: the first reconcile
        // minted and persisted one, sealed at rest.
        let sealed = sqlx::query_scalar::<_, Option<Vec<u8>>>(
            "SELECT secret FROM integrations WHERE id = $1",
        )
        .bind(bound.id)
        .fetch_one(&mut *tx)
        .await
        .expect("read secret");
        assert!(sealed.is_some(), "the first reconcile mints a secret");

        // A running git integration with a published web surface registers a route.
        assert!(
            routes.lookup(bound.id).is_some(),
            "a running Forgejo publishes a proxy route"
        );

        // Git SSOs against Elysium: the reconcile registered exactly one OIDC client
        // under the deterministic id, confidential and first-party.
        assert_eq!(
            confidential_client_count(&mut tx, bound.id).await,
            1,
            "one OIDC client is registered"
        );

        // A second run (a simulated restart) re-converges without duplicating
        // the container: same generation, still RUNNING (the minted secret is
        // reused, so the spec, secret included, is unchanged).
        reconcile_all(&mut tx, &provisioner, &crypto, &routes, app_origin)
            .await
            .expect("re-reconcile");
        let (status, _) = status_of(&mut tx, bound.id).await;
        assert_eq!(status, ReconcileStatus::Running);
        assert_eq!(provisioner.generation(&service), Some(1));
        // Idempotent: no duplicate OIDC client, and the route persists.
        assert_eq!(
            confidential_client_count(&mut tx, bound.id).await,
            1,
            "a re-reconcile reuses the OIDC client, never duplicating it"
        );
        assert!(routes.lookup(bound.id).is_some());

        // An unbound managed integration parks at PENDING, naming the gap.
        let unbound = sqlx::query_as::<_, IntegrationRow>(
            "INSERT INTO integrations (org_id, kind, mode, owner_scope, owner_id, name)
             VALUES ($1, $2, $3, $4, $1, 'Blobs') RETURNING *",
        )
        .bind(bound.org_id)
        .bind(IntegrationKind::Registry.as_db())
        .bind(IntegrationMode::Managed.as_db())
        .bind(OwnerScope::Org.as_db())
        .fetch_one(&mut *tx)
        .await
        .expect("insert unbound integration");
        reconcile_all(&mut tx, &provisioner, &crypto, &routes, app_origin)
            .await
            .expect("reconcile");
        let (status, detail) = status_of(&mut tx, unbound.id).await;
        assert_eq!(status, ReconcileStatus::Pending);
        assert!(detail.contains("blobs"), "names the unbound role: {detail}");

        // Desired stopped converges the container down and persists STOPPED.
        sqlx::query("UPDATE integrations SET desired = $1 WHERE id = $2")
            .bind(DesiredState::Stopped.as_db())
            .bind(bound.id)
            .execute(&mut *tx)
            .await
            .expect("set desired stopped");
        reconcile_integration(
            &mut tx,
            &provisioner,
            &crypto,
            &routes,
            app_origin,
            bound.id,
        )
        .await
        .expect("reconcile one");
        let (status, _) = status_of(&mut tx, bound.id).await;
        assert_eq!(status, ReconcileStatus::Stopped);
        assert_eq!(
            provisioner.inspect(&service).await.expect("inspect"),
            ServiceState::Stopped
        );
        // A stopped integration serves no HTTP: its route is dropped.
        assert!(
            routes.lookup(bound.id).is_none(),
            "stopping a service unregisters its route"
        );

        tx.rollback().await.expect("rollback transaction");
    }

    // --- Host-port allocation against postgres (ignored by default) ----------
    //
    // These prove the persisted allocation itself: uniqueness, in-band, stability,
    // lowest-gap reuse (including free-on-delete), density, and the partial UNIQUE
    // index and band CHECK migration 0030 adds. Everything runs inside a
    // rolled-back transaction, and every assertion is relative (deltas off the
    // first allocation), so a shared test database with pre-existing rows never
    // makes them flaky.

    /// The band's inclusive upper bound, for in-band assertions.
    const BAND_END: u16 = MANAGED_HOST_PORT_BASE + MANAGED_HOST_PORT_SPAN - 1;

    /// Inserts an org and returns its id.
    async fn seed_org(conn: &mut PgConnection) -> Uuid {
        let unique = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ('Port Org', $1) RETURNING id",
        )
        .bind(format!("port-{unique}"))
        .fetch_one(conn)
        .await
        .expect("insert org")
    }

    /// Inserts a minimal org-scoped managed integration with `host_port` NULL.
    ///
    /// Allocation only touches the `host_port` column, so no bindings or config are
    /// needed; the row simply has to exist.
    async fn seed_bare_integration(conn: &mut PgConnection, org: Uuid) -> Uuid {
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO integrations (org_id, kind, mode, owner_scope, owner_id, name)
             VALUES ($1, $2, $3, $4, $1, 'Forge') RETURNING id",
        )
        .bind(org)
        .bind(IntegrationKind::Git.as_db())
        .bind(IntegrationMode::Managed.as_db())
        .bind(OwnerScope::Org.as_db())
        .fetch_one(conn)
        .await
        .expect("insert integration")
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn ensure_host_port_allocates_in_band_and_is_stable() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let org = seed_org(&mut tx).await;

        let a = seed_bare_integration(&mut tx, org).await;
        let port_a = ensure_host_port(&mut tx, a).await.expect("allocate a");
        assert!(
            (MANAGED_HOST_PORT_BASE..=BAND_END).contains(&port_a),
            "{port_a} out of band"
        );

        // Re-calling for the same integration returns the same port: stable across
        // restarts, never reallocated.
        assert_eq!(
            ensure_host_port(&mut tx, a).await.expect("re-read a"),
            port_a,
            "an allocated port is stable"
        );

        // A second integration gets a distinct, higher port (lowest-free after a).
        let b = seed_bare_integration(&mut tx, org).await;
        let port_b = ensure_host_port(&mut tx, b).await.expect("allocate b");
        assert_ne!(port_a, port_b, "distinct integrations get distinct ports");
        assert!(port_b > port_a, "the next allocation takes the next gap");

        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn deleting_an_integration_frees_its_host_port() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let org = seed_org(&mut tx).await;

        let a = seed_bare_integration(&mut tx, org).await;
        let b = seed_bare_integration(&mut tx, org).await;
        let port_a = ensure_host_port(&mut tx, a).await.expect("allocate a");
        let port_b = ensure_host_port(&mut tx, b).await.expect("allocate b");
        assert!(port_a < port_b);

        // Deleting the row (the delete path is a plain cascade DELETE, no soft
        // delete) frees its port: the column travels with the row.
        sqlx::query("DELETE FROM integrations WHERE id = $1")
            .bind(a)
            .execute(&mut *tx)
            .await
            .expect("delete a");

        // A fresh allocation reuses the freed port, now the lowest free again.
        let c = seed_bare_integration(&mut tx, org).await;
        let port_c = ensure_host_port(&mut tx, c).await.expect("allocate c");
        assert_eq!(port_c, port_a, "the freed port is reused");

        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn ensure_host_port_fills_the_lowest_gap() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let org = seed_org(&mut tx).await;

        let a = seed_bare_integration(&mut tx, org).await;
        let b = seed_bare_integration(&mut tx, org).await;
        let c = seed_bare_integration(&mut tx, org).await;
        let port_a = ensure_host_port(&mut tx, a).await.expect("a");
        let port_b = ensure_host_port(&mut tx, b).await.expect("b");
        let port_c = ensure_host_port(&mut tx, c).await.expect("c");
        assert!(port_a < port_b && port_b < port_c);

        // Free the middle port; the next allocation fills that gap, not the top.
        sqlx::query("DELETE FROM integrations WHERE id = $1")
            .bind(b)
            .execute(&mut *tx)
            .await
            .expect("delete b");
        let d = seed_bare_integration(&mut tx, org).await;
        let port_d = ensure_host_port(&mut tx, d).await.expect("d");
        assert_eq!(port_d, port_b, "the lowest gap is filled first");

        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn host_ports_stay_unique_under_density() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let org = seed_org(&mut tx).await;

        // Pack the band densely and assert every port is distinct and in-band: the
        // collision the old hash derivation risked cannot happen here.
        let mut ports = std::collections::BTreeSet::new();
        for _ in 0..32 {
            let id = seed_bare_integration(&mut tx, org).await;
            let port = ensure_host_port(&mut tx, id).await.expect("allocate");
            assert!(
                (MANAGED_HOST_PORT_BASE..=BAND_END).contains(&port),
                "{port} out of band"
            );
            assert!(ports.insert(port), "port {port} was handed out twice");
        }
        assert_eq!(ports.len(), 32, "every allocation is unique");

        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn the_host_port_index_rejects_a_duplicate() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let org = seed_org(&mut tx).await;
        let a = seed_bare_integration(&mut tx, org).await;
        let b = seed_bare_integration(&mut tx, org).await;

        // Force both onto one port: the partial UNIQUE index (migration 0030) makes
        // the second write fail, which is the guarantee the retry loop leans on.
        sqlx::query("UPDATE integrations SET host_port = $1 WHERE id = $2")
            .bind(i32::from(BAND_END))
            .bind(a)
            .execute(&mut *tx)
            .await
            .expect("first claim");
        let dup = sqlx::query("UPDATE integrations SET host_port = $1 WHERE id = $2")
            .bind(i32::from(BAND_END))
            .bind(b)
            .execute(&mut *tx)
            .await;
        assert!(
            is_unique_violation(&dup.expect_err("a duplicate host port must be rejected")),
            "the partial unique index rejects the duplicate"
        );

        tx.rollback().await.expect("rollback");
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn the_host_port_check_rejects_out_of_band_values() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let org = seed_org(&mut tx).await;
        let id = seed_bare_integration(&mut tx, org).await;

        // The band CHECK (migration 0030) rejects a port under the base and one over
        // the top, so no allocation or manual write can escape the band. Each write
        // runs in its own savepoint: a CHECK violation aborts its subtransaction,
        // which ROLLBACK TO SAVEPOINT recovers without poisoning the outer one.
        use sqlx::Acquire;
        for out_of_band in [
            i32::from(MANAGED_HOST_PORT_BASE) - 1,
            i32::from(BAND_END) + 1,
        ] {
            let mut savepoint = tx.begin().await.expect("open savepoint");
            let result = sqlx::query("UPDATE integrations SET host_port = $1 WHERE id = $2")
                .bind(out_of_band)
                .bind(id)
                .execute(&mut *savepoint)
                .await;
            assert!(
                result.is_err(),
                "the CHECK must reject the out-of-band port {out_of_band}"
            );
            savepoint.rollback().await.expect("rollback savepoint");
        }

        tx.rollback().await.expect("rollback");
    }

    // --- Live Docker E2E (ignored, needs the real stack) --------------------
    //
    // This proves a managed Forgejo actually provisions and serves. It runs only
    // with the real provisioner and a Docker engine, so `cargo test` skips it. To
    // run it live, from `backend/`:
    //
    //   1. Switch on the real provisioner + scoped socket-proxy:
    //        docker compose up -d           # brings up postgres + the docker-socket-proxy
    //   2. Point the test at the proxy and a database:
    //        export ELYSIUM_DOCKER_PROXY_URL=http://127.0.0.1:2375
    //        export DATABASE_URL=postgres://…/elysia
    //   3. Run the ignored suite from inside the backend container (where
    //      host.docker.internal resolves to the host gateway):
    //        docker exec elysium-backend cargo test -- --ignored provisions_a_managed_forgejo
    //
    // It provisions the pinned Forgejo image with the templated env (sub-path
    // ROOT_URL, SQLite on the bound Drive, the OIDC login-source parameters), waits
    // for the container to answer, and asserts its web UI serves. The end-to-end
    // "Forgejo fetched Elysium discovery" check needs a reachable live Elysium
    // origin; run the backend and open `/services/{id}/` to complete that manually.

    use tempfile::TempDir;

    use crate::orchestration::provisioner::ProvisionerHandle;

    #[tokio::test]
    #[ignore = "requires a Docker engine via ELYSIUM_DOCKER_PROXY_URL"]
    async fn provisions_a_managed_forgejo_and_serves_its_ui() {
        let Some(proxy_url) = std::env::var("ELYSIUM_DOCKER_PROXY_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
        else {
            return;
        };

        // The Drive root doubles as the provisioner's one allowed bind-mount root,
        // so the reserved `.elysium/git/repos` mount resolves inside it.
        let root = TempDir::new().expect("tempdir");
        let provisioner = ProvisionerHandle::docker(&proxy_url, vec![root.path().to_path_buf()])
            .expect("connect docker backend");

        // A synthetic bound git integration and the pinned Forgejo spec, with the
        // OIDC login-source env templated (as the reconciler would inject it).
        let mut row = managed_row(IntegrationKind::Git);
        row.id = Uuid::new_v4();
        let fs = drive(
            DriveKind::Fs,
            serde_json::json!({ "root": root.path().to_string_lossy() }),
        );
        let bindings = vec![binding(&row, "repos", &fs, "")];
        let drives = HashMap::from([(fs.id, fs)]);
        let oidc = config::OidcClientCreds {
            client_id: git_oidc_client_id(row.id),
            client_secret: ServiceSecret::new("ely_cs_live-e2e"),
        };
        let provisioning = config::Provisioning {
            app_origin: "http://host.docker.internal:8080",
            oidc: Some(&oidc),
        };
        // A fixed in-band port stands in for the persisted allocation this live
        // path never touches a database to make.
        let host_port = MANAGED_HOST_PORT_BASE + 4242;
        let SpecPlan::Ready(spec) = build_spec(
            &row,
            &bindings,
            &drives,
            None,
            &provisioning,
            Some(host_port),
        ) else {
            panic!("the bound git integration plans a ready spec");
        };

        // Provision, assert the UI serves, and always clean the container up.
        let outcome = drive_forgejo(&provisioner, &spec, host_port).await;
        provisioner
            .remove(&spec.name)
            .await
            .expect("cleanup remove");
        outcome.expect("live Forgejo assertions");
    }

    /// Ensures the Forgejo container and polls its published web port for the UI.
    async fn drive_forgejo(
        provisioner: &ProvisionerHandle,
        spec: &ServiceSpec,
        host_port: u16,
    ) -> Result<(), String> {
        provisioner
            .ensure(spec)
            .await
            .map_err(|error| format!("ensure forgejo: {error}"))?;

        // Forgejo runs migrations and starts on first boot; poll the published port.
        let client = reqwest::Client::new();
        let url = format!("http://{MANAGED_UPSTREAM_HOST}:{host_port}/");
        for attempt in 0..60 {
            match client.get(&url).send().await {
                Ok(response) => {
                    let body = response.text().await.unwrap_or_default();
                    // Forgejo's chrome carries its name; its presence proves the
                    // templated env booted a real forge, not an install screen.
                    if body.contains("Forgejo") || body.contains("Powered by Forgejo") {
                        return Ok(());
                    }
                    if attempt == 59 {
                        return Err(format!("forge served, but no Forgejo marker: {body}"));
                    }
                }
                Err(_) if attempt < 59 => {}
                Err(error) => return Err(format!("forge never became reachable: {error}")),
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        Err("forge never served its UI".to_string())
    }
}
