//! Per-kind config templating and secret injection for managed services.
//!
//! A managed integration stores three ingredients: non-secret `config_json`,
//! storage-role bindings, and one sealed service secret (`integrations.secret`,
//! sealed with [`crate::crypto`]). This module turns them into the concrete
//! environment a kind's blessed image expects, filling the env block of the
//! [`ServiceSpec`](crate::orchestration::provisioner::ServiceSpec) the
//! reconciler plans (`docs/integrations.md`).
//!
//! The per-kind knowledge lives in exactly one place: [`kind_template`] is the
//! lookup table (one [`KindTemplate`] per [`IntegrationKind`], the enum rung of
//! `docs/rust.md` M-DI-HIERARCHY), so no other module ever matches on kind to
//! decide what a service's env looks like. Kinds whose image contract is pinned
//! (git -> Forgejo env sections, registry -> `REGISTRY_*`) render their real
//! schema; kinds whose contract the per-service milestones still own (mail,
//! perforce, runner) render the minimal Elysium-defined placeholder documented
//! on their renderers.
//!
//! Secret handling follows three rules. Secrets are unsealed **only at
//! injection time** ([`ensure_service_secret`], called by the reconciler right
//! before spec planning) and exist in memory just long enough to enter the
//! spec. Where a kind consumes a secret and none was stored, one is **minted**
//! (CSPRNG), sealed, and persisted, so the value survives restarts and an
//! admin-supplied credential (the PATCH `credential` field) naturally rotates
//! it: a new sealed secret changes the rendered env, and the reconciler's
//! spec-equality check replaces the container on the next converge. Secrets
//! are **never logged**: they enter the spec as
//! [`EnvValue::secret`](crate::orchestration::provisioner::EnvValue), whose
//! `Debug` renders `<redacted>`, and [`ServiceSecret`] redacts itself the same
//! way.

use std::backtrace::{Backtrace, BacktraceStatus};
use std::collections::BTreeMap;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use sqlx::PgConnection;

use crate::crypto::Crypto;
use crate::orchestration::forward_auth::{HEADER_ELYSIUM_EMAIL, HEADER_ELYSIUM_NAME};
use crate::orchestration::integrations::{
    open_secret, seal_secret, storage_role, IntegrationKind, IntegrationRow,
};
use crate::orchestration::provisioner::EnvValue;

/// Env var present on every managed service: the owning integration's id.
///
/// Gives a container self-identity independent of its labels (which only the
/// engine sees), so a service or sidecar can name itself back to the control
/// plane. Stable for the life of the integration, so it never churns a spec.
const ENV_INTEGRATION_ID: &str = "ELYSIUM_INTEGRATION_ID";

/// Bytes of CSPRNG output behind a minted service secret.
///
/// 32 bytes (256 bits) matches the deployment master key's strength; more adds
/// nothing, less would make the minted token the weakest credential in the
/// chain. Encoded as unpadded url-safe base64, so the value is env-safe.
const MINTED_SECRET_BYTES: usize = 32;

// --- The per-kind template table ---------------------------------------------

/// The env entries a per-kind renderer produces, in insertion order.
type RenderedEntries = Vec<(String, EnvValue)>;

/// A per-kind renderer: the provisioning inputs in, kind-specific env entries out.
type RenderFn = fn(&RenderInput<'_>) -> Result<RenderedEntries, TemplateError>;

/// Everything a per-kind renderer may consume, beyond the stored `config_json`.
///
/// Most kinds only read [`RenderInput::config`]; the git renderer also needs the
/// integration id (for its reverse-proxy sub-path), the deployment origin (its
/// `ROOT_URL` base and the OIDC issuer), and, when provisioned, the OIDC client
/// registered for it. Bundling these keeps [`render_env`] the single builder and
/// leaves every renderer a pure function of its input, so templating stays
/// database-free and unit-tested.
pub(crate) struct RenderInput<'a> {
    /// The integration's stored, non-secret `config_json`.
    config: &'a serde_json::Value,
    /// The integration being provisioned; renderers read its id and kind.
    integration: &'a IntegrationRow,
    /// The provisioning-time context the reconciler resolves (origin, OIDC client).
    provisioning: &'a Provisioning<'a>,
}

/// Provisioning-time context the reconciler resolves before templating.
///
/// Separate from `config_json` because it is not stored on the integration: the
/// origin comes from deployment config and the OIDC client is registered live at
/// reconcile time. Safe to debug-format: the client secret it may carry is a
/// redacting [`ServiceSecret`].
#[derive(Debug, Default)]
pub(crate) struct Provisioning<'a> {
    /// The Elysium app origin: a kind's external `ROOT_URL` base and OIDC issuer.
    pub app_origin: &'a str,
    /// The OIDC client registered for this integration, when its kind SSOs against
    /// Elysium (git today); `None` in the pure "no SSO wired yet" path.
    pub oidc: Option<&'a OidcClientCreds>,
    /// The S3 blob store for a managed registry, resolved when its `blobs` role binds
    /// to an S3 Drive; `None` for FS-backed blobs (the filesystem driver plus the bind
    /// mount the reconciler adds). Like [`Provisioning::oidc`], it is resolved live
    /// (the credential is sealed), not read from the stored config.
    pub registry_blobs: Option<&'a RegistryS3Blobs>,
}

/// The bucket settings and unsealed credential a managed registry's S3 blob store needs.
///
/// The reconciler resolves the registry's `blobs` role to its Drive and, for an S3
/// Drive, parses the bucket settings and unseals the provider credential before
/// templating (mirroring how the git OIDC client is resolved live). It becomes the
/// `REGISTRY_STORAGE_S3_*` env of CNCF distribution's S3 storage driver. The secret
/// access key is a redacting [`ServiceSecret`], so it never leaks through a
/// debug-formatted spec or reconcile log; the access key id (an identifier, like a
/// user name) stays visible.
#[derive(Debug)]
pub(crate) struct RegistryS3Blobs {
    /// The S3 endpoint URL (`REGIONENDPOINT`); empty for AWS's default endpoint.
    pub endpoint: String,
    /// The bucket region (`REGION`); empty when the provider needs none.
    pub region: String,
    /// The bucket the layers land in (`BUCKET`).
    pub bucket: String,
    /// The key prefix under the bucket the registry roots its blobs at
    /// (`ROOTDIRECTORY`): the bound Drive's own prefix joined with the binding's
    /// reserved area, so the registry never collides with other Drive consumers.
    pub root_prefix: String,
    /// The S3 access key id (`ACCESSKEY`).
    pub access_key_id: String,
    /// The S3 secret access key (`SECRETKEY`), redacted in debug output.
    pub secret_access_key: ServiceSecret,
}

/// The OIDC client credentials injected into a managed service's config.
///
/// The client id is public; the secret is a redacting [`ServiceSecret`] so it can
/// never leak through a debug-formatted spec or reconcile log.
#[derive(Debug)]
pub(crate) struct OidcClientCreds {
    /// The registered client's public id (`forgejo-<integration_id>` for git).
    pub client_id: String,
    /// The registered client's confidential secret, redacted in debug output.
    pub client_secret: ServiceSecret,
}

/// How one kind's stored config becomes the env its blessed image expects.
///
/// One instance per [`IntegrationKind`], produced by [`kind_template`]; the
/// fields are data and a function pointer, so the whole table is inspectable
/// and each kind's knowledge stays in its own renderer.
struct KindTemplate {
    /// Env var receiving the integration's service secret, or `None` while the
    /// kind's image contract has no pinned secret slot (mail, until M6).
    secret_env: Option<&'static str>,
    /// Renders the kind-specific, non-secret env from `config_json`.
    render: RenderFn,
}

/// The lookup table: the single place kind maps to its config template.
fn kind_template(kind: IntegrationKind) -> KindTemplate {
    match kind {
        IntegrationKind::Git => KindTemplate {
            // Forgejo's internal signing key (`security.SECRET_KEY`); pinning
            // it keeps sessions and tokens valid across container replacement.
            secret_env: Some("FORGEJO__security__SECRET_KEY"),
            render: render_git,
        },
        IntegrationKind::Mail => KindTemplate {
            // The mail image is still a candidate (docs/integrations.md); M6
            // pins the real schema, including where an admin secret lands.
            secret_env: None,
            render: render_mail,
        },
        IntegrationKind::Registry => KindTemplate {
            // distribution's HTTP secret coordinates upload sessions; left
            // unset the image mints a random one per container, which breaks
            // resumed uploads across replacement.
            secret_env: Some("REGISTRY_HTTP_SECRET"),
            render: render_registry,
        },
        IntegrationKind::Perforce => KindTemplate {
            secret_env: Some("ELYSIUM_SERVICE_TOKEN"),
            render: render_perforce,
        },
        IntegrationKind::Runner => KindTemplate {
            secret_env: Some("ELYSIUM_SERVICE_TOKEN"),
            render: render_runner,
        },
    }
}

/// Whether a kind's template injects a service secret at all.
///
/// The reconciler uses this to skip unsealing (and minting) for kinds without
/// a pinned secret slot, so no secret is ever opened without a consumer.
pub(crate) fn consumes_secret(kind: IntegrationKind) -> bool {
    kind_template(kind).secret_env.is_some()
}

/// Renders the full env block of a managed integration's service spec.
///
/// The shared frame around the per-kind renderers: the Elysium base env, then
/// the kind's own entries, then the service secret injected under the kind's
/// declared slot as an [`EnvValue::secret`] so it never survives a debug
/// rendering. Pure, so template output is unit-tested without a database; the
/// reconciler supplies `secret` from [`ensure_service_secret`].
///
/// # Errors
/// Returns a [`TemplateError`] when `config_json` does not fit the kind's
/// config shape; the reconciler parks the integration `FAILED` with the
/// message rather than panicking.
pub(crate) fn render_env(
    integration: &IntegrationRow,
    secret: Option<&ServiceSecret>,
    provisioning: &Provisioning<'_>,
) -> Result<BTreeMap<String, EnvValue>, TemplateError> {
    let template = kind_template(integration.kind);

    let input = RenderInput {
        config: &integration.config,
        integration,
        provisioning,
    };

    let mut env = BTreeMap::new();
    env.insert(
        ENV_INTEGRATION_ID.to_string(),
        EnvValue::plain(integration.id.to_string()),
    );
    for (name, value) in (template.render)(&input)? {
        env.insert(name, value);
    }
    if let (Some(name), Some(secret)) = (template.secret_env, secret) {
        env.insert(name.to_string(), EnvValue::secret(secret.expose()));
    }
    Ok(env)
}

// --- Per-kind config shapes and renderers ------------------------------------

/// The container port Forgejo's web (and git-over-HTTP) surface listens on.
///
/// Pinned into `FORGEJO__server__HTTP_PORT`. Must equal
/// `reconcile::http_surface_port(Git)`, the port the reconciler publishes and the
/// proxy routes to (#29); the `git_http_port_matches_the_routed_surface` test in
/// `reconcile` guards the two against drift.
const GIT_HTTP_PORT: u16 = 3000;

/// The container port CNCF distribution's HTTP API listens on.
///
/// Pinned into `REGISTRY_HTTP_ADDR`. Must equal `reconcile::http_surface_port(Registry)`,
/// the port the reconciler publishes and the proxy routes to (#29); the
/// `registry_http_port_matches_the_routed_surface` test guards the two against drift.
const REGISTRY_HTTP_PORT: u16 = 5000;

/// The Forgejo login-source name Elysium's OIDC provider is registered under.
///
/// Lowercase and stable: Forgejo builds the OAuth callback path from it
/// (`<ROOT_URL>/user/oauth2/<name>/callback`), so the registered redirect URI
/// (built in [`crate::orchestration::reconcile`]) and the login source must agree
/// on this exact value.
pub(crate) const OIDC_SOURCE_NAME: &str = "elysium";

/// The OAuth scopes Forgejo requests from Elysium's OIDC provider.
///
/// `openid` for authentication, `email`+`profile` so Forgejo can auto-provision a
/// matching account (see [`OIDC_ENV_SCOPES`]). Elysium's provider advertises all
/// three (`SUPPORTED_SCOPES`).
const OIDC_SCOPES: &str = "openid email profile";

/// The source ranges Forgejo trusts to assert reverse-proxy identity headers.
///
/// Forgejo honors the injected `X-WEBAUTH-USER` / `X-Elysium-*` headers only from a
/// source in this list. The backend reaches Forgejo through the host gateway (see
/// [`crate::orchestration::routing::MANAGED_UPSTREAM_HOST`]), so Forgejo sees the
/// request arrive from the Docker network gateway, an RFC 1918 address whose exact
/// value depends on the engine's chosen subnet. Trusting the private ranges (plus
/// loopback) covers that gateway regardless of subnet without ever trusting a public
/// source. The real spoofing barrier is not this list but the proxy stripping every
/// client-supplied identity header ([`crate::orchestration::forward_auth`]): only
/// Elysium can reach Forgejo's published port, and only Elysium sets these headers.
const REVERSE_PROXY_TRUSTED_PROXIES: &str =
    "127.0.0.0/8,::1/128,10.0.0.0/8,172.16.0.0/12,192.168.0.0/16";

/// Env var carrying the OIDC login-source name to the first-boot init.
const OIDC_ENV_SOURCE_NAME: &str = "ELYSIUM_OIDC_SOURCE_NAME";
/// Env var carrying the OIDC discovery URL to the first-boot init.
const OIDC_ENV_DISCOVERY_URL: &str = "ELYSIUM_OIDC_DISCOVERY_URL";
/// Env var carrying the OIDC client id to the first-boot init.
const OIDC_ENV_CLIENT_ID: &str = "ELYSIUM_OIDC_CLIENT_ID";
/// Env var carrying the OIDC client secret to the first-boot init (secret-tagged).
const OIDC_ENV_CLIENT_SECRET: &str = "ELYSIUM_OIDC_CLIENT_SECRET";
/// Env var carrying the OIDC scopes to the first-boot init.
const OIDC_ENV_SCOPES: &str = "ELYSIUM_OIDC_SCOPES";

/// Non-secret `config_json` of a managed git (Forgejo) integration.
///
/// All fields optional: a fresh integration stores `{}` and still provisions.
/// Unknown fields are ignored for forward compatibility; the git milestone
/// extends this shape as the Forgejo surface grows. `ROOT_URL` is derived from
/// the deployment origin and the reverse-proxy sub-path, so it is not a stored
/// field; only `domain` (a display/clone hostname hint) is admin-supplied.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GitConfig {
    /// Public domain the forge serves (`FORGEJO__server__DOMAIN`), when the
    /// operator fronts it with a dedicated hostname; optional.
    domain: String,
}

/// Renders a runnable managed-Forgejo env: `FORGEJO__<section>__<KEY>` plus the
/// Elysium OIDC login-source parameters.
///
/// The forge is self-contained (repos + a SQLite DB on the one POSIX_FS `repos`
/// Drive, so no separate Postgres container is provisioned) and served under the
/// reverse-proxy sub-path `/services/<id>/` (#29), so `ROOT_URL` carries that
/// sub-path and Forgejo generates correct asset, clone, and OAuth-callback URLs.
/// SSO is the entry: self-service local registration is off, and when the
/// reconciler has registered an OIDC client the login-source parameters are
/// templated for the container's first-boot init to feed
/// `forgejo admin auth add-oauth` (Forgejo has no app.ini path for a login
/// source; see `docs/integrations.md`).
fn render_git(input: &RenderInput<'_>) -> Result<RenderedEntries, TemplateError> {
    let config: GitConfig = parse_config(IntegrationKind::Git, input.config)?;
    let base = input.provisioning.app_origin.trim_end_matches('/');
    let root_url = format!("{base}/services/{}/", input.integration.id);
    // SQLite lives on the same POSIX_FS repos Drive as the git objects, under the
    // role's container mount, so the whole forge state is one bound directory.
    let repos = storage_role(IntegrationKind::Git, "repos").expect("git declares a repos role");
    let sqlite_path = format!("{}/forgejo.db", repos.mount_path);

    let mut env = vec![
        // Configured entirely by Elysium; locking the installer keeps the first
        // boot from exposing an unauthenticated setup UI.
        (
            "FORGEJO__security__INSTALL_LOCK".to_string(),
            EnvValue::plain("true"),
        ),
        (
            "FORGEJO__server__ROOT_URL".to_string(),
            EnvValue::plain(root_url),
        ),
        // Plain HTTP behind the proxy; the operator terminates TLS in front of
        // Elysium. The port must match the published/routed surface (#29).
        (
            "FORGEJO__server__PROTOCOL".to_string(),
            EnvValue::plain("http"),
        ),
        (
            "FORGEJO__server__HTTP_PORT".to_string(),
            EnvValue::plain(GIT_HTTP_PORT.to_string()),
        ),
        (
            "FORGEJO__database__DB_TYPE".to_string(),
            EnvValue::plain("sqlite3"),
        ),
        (
            "FORGEJO__database__PATH".to_string(),
            EnvValue::plain(sqlite_path),
        ),
        // SSO is the only entry: no self-service local signup, no signup button.
        (
            "FORGEJO__service__DISABLE_REGISTRATION".to_string(),
            EnvValue::plain("true"),
        ),
        (
            "FORGEJO__service__SHOW_REGISTRATION_BUTTON".to_string(),
            EnvValue::plain("false"),
        ),
        // Reverse-proxy (forward-auth) authentication: Forgejo trusts the identity
        // headers Elysium's proxy injects, so an embedded page opened while signed
        // into Elysium lands authenticated with no visible login (#33). Composes
        // with the OIDC login source above: forward-auth is the zero-flicker web
        // path, OIDC the standards path, both asserting one Elysium identity.
        (
            "FORGEJO__service__ENABLE_REVERSE_PROXY_AUTHENTICATION".to_string(),
            EnvValue::plain("true"),
        ),
        // Auto-provision (JIT) the Forgejo account on the first proxied request from
        // a user it has not seen, keyed by the injected `X-WEBAUTH-USER`.
        (
            "FORGEJO__service__ENABLE_REVERSE_PROXY_AUTO_REGISTRATION".to_string(),
            EnvValue::plain("true"),
        ),
        // Populate the JIT account's email and full name from the injected headers
        // rather than a synthesized placeholder.
        (
            "FORGEJO__service__ENABLE_REVERSE_PROXY_EMAIL".to_string(),
            EnvValue::plain("true"),
        ),
        (
            "FORGEJO__service__ENABLE_REVERSE_PROXY_FULL_NAME".to_string(),
            EnvValue::plain("true"),
        ),
        // The user header stays Forgejo's default `X-WEBAUTH-USER` (which the proxy
        // injects); point the email and full-name headers at the `X-Elysium-*` ones
        // the proxy injects, so a single source of truth drives both sides.
        (
            "FORGEJO__security__REVERSE_PROXY_AUTHENTICATION_EMAIL".to_string(),
            EnvValue::plain(HEADER_ELYSIUM_EMAIL),
        ),
        (
            "FORGEJO__security__REVERSE_PROXY_AUTHENTICATION_FULL_NAME".to_string(),
            EnvValue::plain(HEADER_ELYSIUM_NAME),
        ),
        (
            "FORGEJO__security__REVERSE_PROXY_TRUSTED_PROXIES".to_string(),
            EnvValue::plain(REVERSE_PROXY_TRUSTED_PROXIES),
        ),
    ];
    if !config.domain.trim().is_empty() {
        env.push((
            "FORGEJO__server__DOMAIN".to_string(),
            EnvValue::plain(config.domain.trim()),
        ));
    }

    if let Some(oidc) = input.provisioning.oidc {
        env.extend(oidc_env(base, oidc));
    }
    Ok(env)
}

/// The OIDC login-source env for a managed Forgejo behind Elysium's provider.
///
/// Two parts: the `oauth2_client` account behavior Forgejo honors from app.ini
/// (auto-register + link a Forgejo account to the Elysium identity on first
/// login), and the `ELYSIUM_OIDC_*` login-source parameters the container's
/// first-boot init feeds to `forgejo admin auth add-oauth`. The client secret is
/// secret-tagged so it never survives a debug rendering.
fn oidc_env(base: &str, oidc: &OidcClientCreds) -> RenderedEntries {
    vec![
        // Auto-provision and link a Forgejo account from the Elysium login.
        (
            "FORGEJO__oauth2_client__ENABLE_AUTO_REGISTRATION".to_string(),
            EnvValue::plain("true"),
        ),
        (
            "FORGEJO__oauth2_client__ACCOUNT_LINKING".to_string(),
            EnvValue::plain("auto"),
        ),
        (
            "FORGEJO__oauth2_client__UPDATE_AVATAR".to_string(),
            EnvValue::plain("true"),
        ),
        (
            "FORGEJO__oauth2_client__USERNAME".to_string(),
            EnvValue::plain("nickname"),
        ),
        // The login-source parameters the first-boot init registers against.
        (
            OIDC_ENV_SOURCE_NAME.to_string(),
            EnvValue::plain(OIDC_SOURCE_NAME),
        ),
        (
            OIDC_ENV_DISCOVERY_URL.to_string(),
            EnvValue::plain(format!("{base}/.well-known/openid-configuration")),
        ),
        (
            OIDC_ENV_CLIENT_ID.to_string(),
            EnvValue::plain(oidc.client_id.as_str()),
        ),
        (OIDC_ENV_SCOPES.to_string(), EnvValue::plain(OIDC_SCOPES)),
        (
            OIDC_ENV_CLIENT_SECRET.to_string(),
            EnvValue::secret(oidc.client_secret.expose()),
        ),
    ]
}

/// Placeholder: the mail image contract is not pinned yet.
///
/// The mail server candidates (docs/integrations.md) configure differently
/// enough that inventing env names now would just be churn; M6 pins the image
/// and this renderer grows the real schema (hostname, domains, admin secret).
/// Until then a managed mail integration carries only the base env.
fn render_mail(input: &RenderInput<'_>) -> Result<RenderedEntries, TemplateError> {
    // Parsed (and rejected when malformed) even though no fields are consumed
    // yet, so a bad stored config surfaces now instead of on the M6 upgrade.
    let _: EmptyConfig = parse_config(IntegrationKind::Mail, input.config)?;
    Ok(Vec::new())
}

/// Renders a runnable CNCF distribution env: `REGISTRY_<SECTION>_<KEY>` overrides.
///
/// The registry listens on [`REGISTRY_HTTP_PORT`] (the routed surface), pins upload
/// sessions with the sealed `REGISTRY_HTTP_SECRET` (injected by [`render_env`]), and
/// enables blob/manifest deletes so image garbage collection works through the API.
/// It runs with **no registry-level auth** (`REGISTRY_AUTH` unset): the only route to
/// its published host port is Elysium's reverse proxy, which authenticates every
/// request and enforces the token's `RegistryPull`/`RegistryPush` scope before
/// forwarding (see [`crate::orchestration::forward_auth`]). That proxy is the gate;
/// see `docs/integrations.md` for the trust boundary.
///
/// Blobs bind to an `OBJECT_STORE` Drive (the capability gate in
/// [`crate::orchestration::integrations`] already rejects SFTP/WebDAV, so binding to a
/// non-object-store Drive never reaches here). The storage driver follows the bound
/// Drive's kind: an S3 Drive drives the `REGISTRY_STORAGE_S3_*` family from
/// [`Provisioning::registry_blobs`] (the reconciler resolves and unseals it); an
/// FS-backed (or not-yet-resolved) binding uses the filesystem driver rooted at the
/// `blobs` role's container mount, which the reconciler bind-mounts to the host Drive.
fn render_registry(input: &RenderInput<'_>) -> Result<RenderedEntries, TemplateError> {
    let _: EmptyConfig = parse_config(IntegrationKind::Registry, input.config)?;

    let mut env = vec![
        // Listen where the reconciler publishes and the proxy dials (#29).
        (
            "REGISTRY_HTTP_ADDR".to_string(),
            EnvValue::plain(format!(":{REGISTRY_HTTP_PORT}")),
        ),
        // Let blob/manifest deletes through so image GC and retention work via the API.
        (
            "REGISTRY_STORAGE_DELETE_ENABLED".to_string(),
            EnvValue::plain("true"),
        ),
    ];

    match input.provisioning.registry_blobs {
        // S3-backed blobs: the provider-native S3 storage driver, keys sealed.
        Some(s3) => env.extend(registry_s3_env(s3)),
        // FS-backed (or not-yet-resolved) blobs: the filesystem driver rooted at the
        // role's container mount, which the storage-role table and the bind mount agree
        // on by construction. Setting the filesystem root and no S3 keys picks the
        // filesystem driver; the two drivers are never set together.
        None => {
            let blobs = storage_role(IntegrationKind::Registry, "blobs")
                .expect("the registry kind declares a blobs role");
            env.push((
                "REGISTRY_STORAGE_FILESYSTEM_ROOTDIRECTORY".to_string(),
                EnvValue::plain(blobs.mount_path),
            ));
        }
    }
    Ok(env)
}

/// The `REGISTRY_STORAGE_S3_*` env for distribution's S3 storage driver.
///
/// Renders the bucket settings and the unsealed credential; `ACCESSKEY` and
/// `SECRETKEY` enter as [`EnvValue::secret`] so neither survives a debug rendering.
/// `REGION`, `REGIONENDPOINT`, and `ROOTDIRECTORY` are emitted only when the bound
/// Drive sets them, so an empty value falls back to the provider default rather than
/// pinning an empty string (matching how the storage provider builds its operator).
fn registry_s3_env(s3: &RegistryS3Blobs) -> RenderedEntries {
    let mut env = vec![
        (
            "REGISTRY_STORAGE_S3_BUCKET".to_string(),
            EnvValue::plain(s3.bucket.clone()),
        ),
        (
            "REGISTRY_STORAGE_S3_ACCESSKEY".to_string(),
            EnvValue::secret(s3.access_key_id.clone()),
        ),
        (
            "REGISTRY_STORAGE_S3_SECRETKEY".to_string(),
            EnvValue::secret(s3.secret_access_key.expose()),
        ),
    ];
    if !s3.region.is_empty() {
        env.push((
            "REGISTRY_STORAGE_S3_REGION".to_string(),
            EnvValue::plain(s3.region.clone()),
        ));
    }
    if !s3.endpoint.is_empty() {
        env.push((
            "REGISTRY_STORAGE_S3_REGIONENDPOINT".to_string(),
            EnvValue::plain(s3.endpoint.clone()),
        ));
    }
    if !s3.root_prefix.is_empty() {
        env.push((
            "REGISTRY_STORAGE_S3_ROOTDIRECTORY".to_string(),
            EnvValue::plain(s3.root_prefix.clone()),
        ));
    }
    env
}

/// Placeholder: the Astraval image is not published yet.
///
/// `ELYSIUM_SERVICE_TOKEN` is the intended control-plane handshake for
/// JalapenoLabs images, flowing through the same seam as the pinned kinds; the
/// Astraval milestone pins the full schema alongside the image itself.
fn render_perforce(input: &RenderInput<'_>) -> Result<RenderedEntries, TemplateError> {
    let _: EmptyConfig = parse_config(IntegrationKind::Perforce, input.config)?;
    Ok(Vec::new())
}

/// Placeholder: the runner image is not published yet (see [`render_perforce`]).
fn render_runner(input: &RenderInput<'_>) -> Result<RenderedEntries, TemplateError> {
    let _: EmptyConfig = parse_config(IntegrationKind::Runner, input.config)?;
    Ok(Vec::new())
}

/// The config shape of kinds with no pinned fields yet: any JSON object.
#[derive(Debug, Default, Deserialize)]
struct EmptyConfig {}

/// Deserializes a kind's `config_json` into its config struct.
fn parse_config<T: DeserializeOwned>(
    kind: IntegrationKind,
    config: &serde_json::Value,
) -> Result<T, TemplateError> {
    serde_json::from_value(config.clone())
        .map_err(|source| TemplateError::new(kind, format!("malformed config_json: {source}")))
}

// --- The service secret ------------------------------------------------------

/// An integration's unsealed service secret, alive only between unseal and injection.
///
/// Wraps the plaintext so it cannot leak through a debug rendering (`Debug`
/// prints `ServiceSecret(<redacted>)`, like [`crate::crypto::Crypto`]); the
/// only way to read the material is the explicit [`ServiceSecret::expose`]
/// call [`render_env`] makes when placing it into the spec.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ServiceSecret(String);

impl ServiceSecret {
    /// Wraps an already-unsealed secret value.
    pub(crate) fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The plaintext secret, for injection into a spec.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

// Redacts the material (see the `service_secret_debug_redacts` test).
impl std::fmt::Debug for ServiceSecret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceSecret(<redacted>)")
    }
}

/// The outcome of resolving an integration's service secret.
///
/// Mirrors the reconciler's `SpecPlan` split: a resolution problem is a parked
/// `FAILED` status with a reason, never a bubbled error or a panic.
#[derive(Debug)]
pub(crate) enum SecretPlan {
    /// The kind's template injects no secret; nothing to unseal or mint.
    NotNeeded,
    /// The secret is unsealed and ready to inject.
    Ready(ServiceSecret),
    /// The stored secret cannot be used (wrong key, tampering, bad encoding).
    Broken(String),
}

/// Resolves the service secret to inject, minting and persisting one if needed.
///
/// The reconciler calls this right before spec planning, which is the only
/// moment the sealed blob is opened. A kind whose template consumes no secret
/// resolves to [`SecretPlan::NotNeeded`] without touching the blob. Otherwise
/// the stored secret (an admin-supplied credential, or a previously minted
/// value) is unsealed; when none is stored, a fresh CSPRNG value is minted,
/// sealed against the integration id, and persisted, so the secret is stable
/// across restarts until an admin rotates it through the credential field.
///
/// # Errors
/// Returns an error only when the database write for a minted secret fails;
/// an unusable stored secret is reported as [`SecretPlan::Broken`] instead.
pub(crate) async fn ensure_service_secret(
    conn: &mut PgConnection,
    crypto: &Crypto,
    integration: &IntegrationRow,
) -> sqlx::Result<SecretPlan> {
    if !consumes_secret(integration.kind) {
        return Ok(SecretPlan::NotNeeded);
    }

    if let Some(blob) = &integration.secret {
        let plaintext = match open_secret(crypto, integration.id, blob) {
            Ok(plaintext) => plaintext,
            Err(error) => {
                return Ok(SecretPlan::Broken(format!(
                    "the stored service secret cannot be opened: {error}"
                )));
            }
        };
        return Ok(match String::from_utf8(plaintext) {
            Ok(secret) => SecretPlan::Ready(ServiceSecret(secret)),
            // Env vars are strings; a non-UTF-8 credential cannot be injected.
            Err(_) => SecretPlan::Broken(
                "the stored service secret is not valid UTF-8 and cannot be injected as an \
                 environment variable"
                    .to_string(),
            ),
        });
    }

    let minted = mint_secret();
    let sealed = seal_secret(crypto, integration.id, minted.as_bytes());
    sqlx::query("UPDATE integrations SET secret = $1, updated_at = now() WHERE id = $2")
        .bind(sealed)
        .bind(integration.id)
        .execute(conn)
        .await?;
    Ok(SecretPlan::Ready(ServiceSecret(minted)))
}

/// Mints a fresh service secret: CSPRNG bytes, env-safe base64.
fn mint_secret() -> String {
    let mut bytes = [0u8; MINTED_SECRET_BYTES];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

// --- Errors ------------------------------------------------------------------

/// An integration's stored config cannot be templated for its kind.
///
/// The canonical error struct of the templating seam: the kind whose template
/// rejected the config, a user-facing message, and a backtrace captured at
/// construction (populated only when `RUST_BACKTRACE` asks for it). The
/// reconciler renders it into the row's `FAILED` `status_detail`.
#[derive(Debug)]
pub(crate) struct TemplateError {
    kind: IntegrationKind,
    message: String,
    backtrace: Backtrace,
}

impl TemplateError {
    /// A `kind` template rejected its input for the reason in `message`.
    fn new(kind: IntegrationKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            backtrace: Backtrace::capture(),
        }
    }
}

impl std::fmt::Display for TemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cannot template the {} service config: {}",
            self.kind.slug(),
            self.message
        )?;
        if self.backtrace.status() == BacktraceStatus::Captured {
            write!(f, "\n{}", self.backtrace)?;
        }
        Ok(())
    }
}

impl std::error::Error for TemplateError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestration::integrations::{
        DesiredState, IntegrationMode, IntegrationVisibility, OwnerScope, ReconcileStatus,
    };
    use chrono::Utc;
    use uuid::Uuid;

    /// A managed row of `kind` carrying `config`, for pure rendering tests.
    fn managed_row(kind: IntegrationKind, config: serde_json::Value) -> IntegrationRow {
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
            config,
            secret: None,
            desired: DesiredState::Running,
            status: ReconcileStatus::Pending,
            status_detail: String::new(),
            created_by: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// A known secret value tests can assert on (present or redacted).
    fn known_secret() -> ServiceSecret {
        ServiceSecret("s3cr3t-admin-token-0f9d2c81".to_string())
    }

    /// Known OIDC client credentials tests can assert on (id visible, secret redacted).
    fn known_oidc() -> OidcClientCreds {
        OidcClientCreds {
            client_id: "forgejo-11111111-2222-3333-4444-555555555555".to_string(),
            client_secret: ServiceSecret("ely_cs_test-client-secret".to_string()),
        }
    }

    // --- render_env: the pinned kinds ----------------------------------------

    #[test]
    fn git_template_renders_a_runnable_forgejo_env() {
        let row = managed_row(
            IntegrationKind::Git,
            serde_json::json!({ "domain": "git.example.com" }),
        );
        let secret = known_secret();
        let oidc = known_oidc();
        let provisioning = Provisioning {
            app_origin: "https://elysium.example.com",
            oidc: Some(&oidc),
            ..Provisioning::default()
        };
        let env = render_env(&row, Some(&secret), &provisioning).expect("valid config renders");

        assert_eq!(env[ENV_INTEGRATION_ID].expose(), row.id.to_string());
        assert_eq!(env["FORGEJO__security__INSTALL_LOCK"].expose(), "true");
        // ROOT_URL carries the reverse-proxy sub-path so generated URLs are correct.
        assert_eq!(
            env["FORGEJO__server__ROOT_URL"].expose(),
            format!("https://elysium.example.com/services/{}/", row.id)
        );
        assert_eq!(env["FORGEJO__server__PROTOCOL"].expose(), "http");
        assert_eq!(env["FORGEJO__server__HTTP_PORT"].expose(), "3000");
        assert_eq!(env["FORGEJO__server__DOMAIN"].expose(), "git.example.com");
        // SQLite on the repos Drive mount; no separate Postgres container.
        assert_eq!(env["FORGEJO__database__DB_TYPE"].expose(), "sqlite3");
        assert_eq!(env["FORGEJO__database__PATH"].expose(), "/data/forgejo.db");
        // SSO is the only entry: local registration is off.
        assert_eq!(
            env["FORGEJO__service__DISABLE_REGISTRATION"].expose(),
            "true"
        );

        // The OIDC login source points at Elysium's discovery, carries the injected
        // client id, and keeps the client secret redacted.
        assert_eq!(env[OIDC_ENV_SOURCE_NAME].expose(), "elysium");
        assert_eq!(
            env[OIDC_ENV_DISCOVERY_URL].expose(),
            "https://elysium.example.com/.well-known/openid-configuration"
        );
        assert_eq!(env[OIDC_ENV_CLIENT_ID].expose(), oidc.client_id);
        assert_eq!(env[OIDC_ENV_SCOPES].expose(), "openid email profile");
        let client_secret = &env[OIDC_ENV_CLIENT_SECRET];
        assert_eq!(client_secret.expose(), oidc.client_secret.expose());
        assert!(client_secret.is_secret());
        assert_eq!(
            env["FORGEJO__oauth2_client__ENABLE_AUTO_REGISTRATION"].expose(),
            "true"
        );
        assert_eq!(
            env["FORGEJO__oauth2_client__ACCOUNT_LINKING"].expose(),
            "auto"
        );

        // The service secret still lands in Forgejo's signing-key slot, tagged secret.
        let key = &env["FORGEJO__security__SECRET_KEY"];
        assert_eq!(key.expose(), secret.expose());
        assert!(key.is_secret());
    }

    #[test]
    fn git_template_enables_reverse_proxy_forward_auth() {
        // Forgejo must trust the identity headers the proxy injects (#33), and JIT
        // the account on first proxied request. A fresh integration (`{}`, no OIDC
        // client) still renders the full reverse-proxy-auth block.
        let row = managed_row(IntegrationKind::Git, serde_json::json!({}));
        let env =
            render_env(&row, Some(&known_secret()), &Provisioning::default()).expect("renders");

        assert_eq!(
            env["FORGEJO__service__ENABLE_REVERSE_PROXY_AUTHENTICATION"].expose(),
            "true"
        );
        assert_eq!(
            env["FORGEJO__service__ENABLE_REVERSE_PROXY_AUTO_REGISTRATION"].expose(),
            "true"
        );
        assert_eq!(
            env["FORGEJO__service__ENABLE_REVERSE_PROXY_EMAIL"].expose(),
            "true"
        );
        assert_eq!(
            env["FORGEJO__service__ENABLE_REVERSE_PROXY_FULL_NAME"].expose(),
            "true"
        );
        // The configured email/full-name header names match the headers the proxy
        // injects, so the trusted-header contract has one source of truth.
        assert_eq!(
            env["FORGEJO__security__REVERSE_PROXY_AUTHENTICATION_EMAIL"].expose(),
            HEADER_ELYSIUM_EMAIL
        );
        assert_eq!(
            env["FORGEJO__security__REVERSE_PROXY_AUTHENTICATION_FULL_NAME"].expose(),
            HEADER_ELYSIUM_NAME
        );
        // The trusted proxies cover the Docker gateway (private ranges) and loopback.
        let trusted = env["FORGEJO__security__REVERSE_PROXY_TRUSTED_PROXIES"].expose();
        assert!(trusted.contains("172.16.0.0/12"));
        assert!(trusted.contains("127.0.0.0/8"));
    }

    #[test]
    fn git_template_omits_the_oidc_source_without_a_registered_client() {
        // A fresh integration stores `{}`; with no OIDC client wired yet, the base
        // Forgejo env still renders but the login-source env is absent. ROOT_URL is
        // derived (not a stored field), so it is always present.
        let row = managed_row(IntegrationKind::Git, serde_json::json!({}));
        let provisioning = Provisioning {
            app_origin: "https://elysium.example.com/",
            oidc: None,
            ..Provisioning::default()
        };
        let env =
            render_env(&row, Some(&known_secret()), &provisioning).expect("empty config renders");

        assert!(env.contains_key(ENV_INTEGRATION_ID));
        assert_eq!(env["FORGEJO__security__INSTALL_LOCK"].expose(), "true");
        assert!(env.contains_key("FORGEJO__security__SECRET_KEY"));
        // A trailing slash on the origin is normalized, not doubled.
        assert_eq!(
            env["FORGEJO__server__ROOT_URL"].expose(),
            format!("https://elysium.example.com/services/{}/", row.id)
        );
        // No admin domain, no OIDC source.
        assert!(!env.contains_key("FORGEJO__server__DOMAIN"));
        assert!(!env.contains_key(OIDC_ENV_CLIENT_ID));
        assert!(!env.contains_key("FORGEJO__oauth2_client__ACCOUNT_LINKING"));
    }

    #[test]
    fn registry_template_fs_blobs_pins_the_root_addr_and_http_secret() {
        // Default provisioning (no S3 blobs resolved): the filesystem driver path.
        let row = managed_row(IntegrationKind::Registry, serde_json::json!({}));
        let secret = known_secret();
        let env = render_env(&row, Some(&secret), &Provisioning::default()).expect("renders");

        // Listens on the routed surface port, and lets deletes through.
        assert_eq!(env["REGISTRY_HTTP_ADDR"].expose(), ":5000");
        assert_eq!(env["REGISTRY_STORAGE_DELETE_ENABLED"].expose(), "true");
        // The filesystem driver root equals the blobs role's declared container mount,
        // so the storage-role table and the image agree by construction.
        assert_eq!(
            env["REGISTRY_STORAGE_FILESYSTEM_ROOTDIRECTORY"].expose(),
            storage_role(IntegrationKind::Registry, "blobs")
                .expect("registry declares blobs")
                .mount_path
        );
        // FS blobs never emit the S3 driver: the two drivers are mutually exclusive.
        assert!(!env.contains_key("REGISTRY_STORAGE_S3_BUCKET"));
        let http_secret = &env["REGISTRY_HTTP_SECRET"];
        assert_eq!(http_secret.expose(), secret.expose());
        assert!(http_secret.is_secret());
    }

    #[test]
    fn registry_template_s3_blobs_renders_the_full_driver_with_secrets_redacted() {
        // An S3-backed blobs binding drives the provider-native S3 storage driver.
        let row = managed_row(IntegrationKind::Registry, serde_json::json!({}));
        let secret = known_secret();
        let blobs = RegistryS3Blobs {
            endpoint: "https://s3.example.com".to_string(),
            region: "us-east-1".to_string(),
            bucket: "elysium-layers".to_string(),
            root_prefix: "team/.elysium/registry/blobs".to_string(),
            access_key_id: "AKIAEXAMPLE".to_string(),
            secret_access_key: ServiceSecret::new("s3-secret-access-key-value"),
        };
        let provisioning = Provisioning {
            registry_blobs: Some(&blobs),
            ..Provisioning::default()
        };
        let env = render_env(&row, Some(&secret), &provisioning).expect("renders");

        // The full REGISTRY_STORAGE_S3_* family, from the bound Drive's config.
        assert_eq!(env["REGISTRY_STORAGE_S3_BUCKET"].expose(), "elysium-layers");
        assert_eq!(env["REGISTRY_STORAGE_S3_REGION"].expose(), "us-east-1");
        assert_eq!(
            env["REGISTRY_STORAGE_S3_REGIONENDPOINT"].expose(),
            "https://s3.example.com"
        );
        assert_eq!(
            env["REGISTRY_STORAGE_S3_ROOTDIRECTORY"].expose(),
            "team/.elysium/registry/blobs"
        );
        // The access key id is visible; both key fields are secret-tagged.
        let access = &env["REGISTRY_STORAGE_S3_ACCESSKEY"];
        assert_eq!(access.expose(), "AKIAEXAMPLE");
        assert!(access.is_secret());
        let secret_key = &env["REGISTRY_STORAGE_S3_SECRETKEY"];
        assert_eq!(secret_key.expose(), "s3-secret-access-key-value");
        assert!(secret_key.is_secret());
        // The S3 driver is exclusive: the filesystem root is never set alongside it.
        assert!(!env.contains_key("REGISTRY_STORAGE_FILESYSTEM_ROOTDIRECTORY"));
        assert_eq!(env["REGISTRY_HTTP_ADDR"].expose(), ":5000");

        // The rendered env (what a spec carries into logs) redacts the secret key.
        let rendered = format!("{env:?}");
        assert!(!rendered.contains("s3-secret-access-key-value"));
        assert!(rendered.contains("REGISTRY_STORAGE_S3_SECRETKEY"));
        assert!(rendered.contains("<redacted>"));
    }

    #[test]
    fn registry_template_s3_blobs_omits_empty_optional_fields() {
        // Only the bucket is required; empty region/endpoint/prefix fall back to the
        // provider default rather than pinning an empty string.
        let row = managed_row(IntegrationKind::Registry, serde_json::json!({}));
        let blobs = RegistryS3Blobs {
            endpoint: String::new(),
            region: String::new(),
            bucket: "only-bucket".to_string(),
            root_prefix: String::new(),
            access_key_id: "AKIA".to_string(),
            secret_access_key: ServiceSecret::new("s"),
        };
        let provisioning = Provisioning {
            registry_blobs: Some(&blobs),
            ..Provisioning::default()
        };
        let env = render_env(&row, Some(&known_secret()), &provisioning).expect("renders");

        assert_eq!(env["REGISTRY_STORAGE_S3_BUCKET"].expose(), "only-bucket");
        assert!(!env.contains_key("REGISTRY_STORAGE_S3_REGION"));
        assert!(!env.contains_key("REGISTRY_STORAGE_S3_REGIONENDPOINT"));
        assert!(!env.contains_key("REGISTRY_STORAGE_S3_ROOTDIRECTORY"));
    }

    #[test]
    fn registry_http_port_matches_the_routed_surface() {
        // The templated REGISTRY_HTTP_ADDR port must equal the container surface port
        // the reconciler publishes and the proxy dials, or the registry listens where
        // the proxy does not reach.
        assert_eq!(
            Some(REGISTRY_HTTP_PORT),
            crate::orchestration::reconcile::http_surface_port(IntegrationKind::Registry)
        );
    }

    // --- render_env: the placeholder kinds -----------------------------------

    #[test]
    fn mail_template_is_the_documented_placeholder() {
        // No pinned image contract yet (M6): base env only, no secret slot.
        let row = managed_row(IntegrationKind::Mail, serde_json::json!({}));
        let env = render_env(&row, None, &Provisioning::default()).expect("renders");
        assert_eq!(env.len(), 1);
        assert!(env.contains_key(ENV_INTEGRATION_ID));
        assert!(!consumes_secret(IntegrationKind::Mail));
    }

    #[test]
    fn jalapenolabs_kinds_inject_the_service_token() {
        for kind in [IntegrationKind::Perforce, IntegrationKind::Runner] {
            let row = managed_row(kind, serde_json::json!({}));
            let secret = known_secret();
            let env = render_env(&row, Some(&secret), &Provisioning::default()).expect("renders");

            let token = &env["ELYSIUM_SERVICE_TOKEN"];
            assert_eq!(token.expose(), secret.expose());
            assert!(token.is_secret(), "{kind:?} token must be secret-tagged");
            assert!(consumes_secret(kind));
        }
    }

    #[test]
    fn every_kind_renders_an_empty_config() {
        // `{}` is what a fresh integration stores; no kind may reject it.
        for kind in IntegrationKind::ALL {
            let row = managed_row(kind, serde_json::json!({}));
            let env =
                render_env(&row, Some(&known_secret()), &Provisioning::default()).expect("renders");
            assert!(env.contains_key(ENV_INTEGRATION_ID), "{kind:?} base env");
        }
    }

    // --- Malformed config ----------------------------------------------------

    #[test]
    fn malformed_config_is_a_clear_error_not_a_panic() {
        // A field of the wrong type fails the template with a message naming
        // the kind; the reconciler parks the row FAILED with it.
        let row = managed_row(IntegrationKind::Git, serde_json::json!({ "domain": 123 }));
        let error =
            render_env(&row, None, &Provisioning::default()).expect_err("bad config rejected");
        let rendered = error.to_string();
        assert!(rendered.contains("git"), "names the kind: {rendered}");
        assert!(
            rendered.contains("malformed config_json"),
            "names the problem: {rendered}"
        );
    }

    // --- Redaction -----------------------------------------------------------

    #[test]
    fn service_secret_debug_redacts() {
        let secret = known_secret();
        let rendered = format!("{secret:?}");
        assert_eq!(rendered, "ServiceSecret(<redacted>)");
        assert!(!rendered.contains(secret.expose()));
    }

    #[test]
    fn rendered_env_debug_never_contains_the_secret() {
        // The full rendered env (what a spec carries into logs and errors)
        // must redact the secret while keeping keys and plain values visible.
        let row = managed_row(
            IntegrationKind::Git,
            serde_json::json!({ "domain": "git.example.com" }),
        );
        let secret = known_secret();
        let oidc = known_oidc();
        let provisioning = Provisioning {
            app_origin: "https://elysium.example.com",
            oidc: Some(&oidc),
            ..Provisioning::default()
        };
        let env = render_env(&row, Some(&secret), &provisioning).expect("renders");

        let rendered = format!("{env:?}");
        // Neither the service secret nor the OIDC client secret leaks; keys and
        // plain values stay visible.
        assert!(!rendered.contains(secret.expose()));
        assert!(!rendered.contains(oidc.client_secret.expose()));
        assert!(rendered.contains("FORGEJO__security__SECRET_KEY"));
        assert!(rendered.contains(OIDC_ENV_CLIENT_SECRET));
        assert!(rendered.contains("git.example.com"));
        assert!(rendered.contains("<redacted>"));
    }

    // --- Minting -------------------------------------------------------------

    #[test]
    fn minted_secrets_are_unique_and_env_safe() {
        let first = mint_secret();
        let second = mint_secret();
        assert_ne!(first, second, "CSPRNG output must differ per mint");
        // 32 bytes of unpadded url-safe base64 is 43 chars of [A-Za-z0-9_-].
        assert_eq!(first.len(), 43);
        assert!(first
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    // --- ensure_service_secret against postgres (ignored by default) ---------
    //
    // Run with `cargo test -- --ignored` and a DATABASE_URL; everything happens
    // inside a rolled-back transaction, like the reconciler's own DB tests.

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

    /// A fixed 32-byte key for deterministic crypto in DB tests.
    fn test_crypto() -> Crypto {
        Crypto::new(&[5u8; 32]).expect("32-byte key is valid")
    }

    /// Seeds an org and a managed integration of `kind`, returning the row.
    async fn seed_integration(conn: &mut PgConnection, kind: IntegrationKind) -> IntegrationRow {
        let unique = Uuid::new_v4();
        let org = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO orgs (name, slug) VALUES ('Secret Org', $1) RETURNING id",
        )
        .bind(format!("sec-{unique}"))
        .fetch_one(&mut *conn)
        .await
        .expect("insert org");
        sqlx::query_as::<_, IntegrationRow>(
            "INSERT INTO integrations (org_id, kind, mode, owner_scope, owner_id, name)
             VALUES ($1, $2, $3, $4, $1, 'Svc') RETURNING *",
        )
        .bind(org)
        .bind(kind.as_db())
        .bind(IntegrationMode::Managed.as_db())
        .bind(OwnerScope::Org.as_db())
        .fetch_one(conn)
        .await
        .expect("insert integration")
    }

    /// Reloads an integration row by id.
    async fn reload(conn: &mut PgConnection, id: Uuid) -> IntegrationRow {
        sqlx::query_as::<_, IntegrationRow>("SELECT * FROM integrations WHERE id = $1")
            .bind(id)
            .fetch_one(conn)
            .await
            .expect("reload integration")
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn ensure_service_secret_mints_persists_and_reuses() {
        let pool = connect_test_pool().await;
        let mut tx = pool.begin().await.expect("begin transaction");
        let crypto = test_crypto();

        let row = seed_integration(&mut tx, IntegrationKind::Git).await;
        assert!(row.secret.is_none(), "fresh rows store no secret");

        // First resolution mints a secret and seals it onto the row.
        let SecretPlan::Ready(minted) = ensure_service_secret(&mut tx, &crypto, &row)
            .await
            .expect("resolve")
        else {
            panic!("a git integration consumes a secret");
        };
        let stored = reload(&mut tx, row.id).await;
        let blob = stored.secret.as_ref().expect("minted secret was persisted");
        // At rest the blob is sealed: it does not contain the plaintext, and
        // it opens back to exactly the minted value.
        assert!(!blob
            .windows(minted.expose().len())
            .any(|window| window == minted.expose().as_bytes()));
        assert_eq!(
            open_secret(&crypto, row.id, blob).expect("opens"),
            minted.expose().as_bytes()
        );

        // A second resolution reuses the stored secret instead of re-minting.
        let SecretPlan::Ready(reused) = ensure_service_secret(&mut tx, &crypto, &stored)
            .await
            .expect("resolve again")
        else {
            panic!("stored secret resolves");
        };
        assert_eq!(reused, minted);

        // A kind without a secret slot never touches or creates one.
        let mail = seed_integration(&mut tx, IntegrationKind::Mail).await;
        assert!(matches!(
            ensure_service_secret(&mut tx, &crypto, &mail)
                .await
                .expect("resolve mail"),
            SecretPlan::NotNeeded
        ));
        assert!(reload(&mut tx, mail.id).await.secret.is_none());

        // A blob sealed under a different id (tampering / replay) is Broken,
        // reported as a reason rather than an error or panic.
        let mut tampered = stored.clone();
        tampered.secret = Some(seal_secret(&crypto, Uuid::new_v4(), b"stolen"));
        let SecretPlan::Broken(detail) = ensure_service_secret(&mut tx, &crypto, &tampered)
            .await
            .expect("resolve tampered")
        else {
            panic!("a foreign blob must not open");
        };
        assert!(detail.contains("cannot be opened"), "detail: {detail}");

        tx.rollback().await.expect("rollback transaction");
    }
}
