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

/// A per-kind renderer: `config_json` in, kind-specific env entries out.
type RenderFn = fn(&serde_json::Value) -> Result<RenderedEntries, TemplateError>;

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
) -> Result<BTreeMap<String, EnvValue>, TemplateError> {
    let template = kind_template(integration.kind);

    let mut env = BTreeMap::new();
    env.insert(
        ENV_INTEGRATION_ID.to_string(),
        EnvValue::plain(integration.id.to_string()),
    );
    for (name, value) in (template.render)(&integration.config)? {
        env.insert(name, value);
    }
    if let (Some(name), Some(secret)) = (template.secret_env, secret) {
        env.insert(name.to_string(), EnvValue::secret(secret.expose()));
    }
    Ok(env)
}

// --- Per-kind config shapes and renderers ------------------------------------

/// Non-secret `config_json` of a managed git (Forgejo) integration.
///
/// All fields optional: a fresh integration stores `{}` and still provisions.
/// Unknown fields are ignored for forward compatibility; M4 (the git
/// milestone) extends this shape as the Forgejo surface grows.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GitConfig {
    /// Public domain the forge serves (`FORGEJO__server__DOMAIN`).
    domain: String,
    /// Full external URL of the forge (`FORGEJO__server__ROOT_URL`).
    root_url: String,
}

/// Forgejo env: `FORGEJO__<section>__<KEY>` entries, per its container docs.
fn render_git(config: &serde_json::Value) -> Result<RenderedEntries, TemplateError> {
    let config: GitConfig = parse_config(IntegrationKind::Git, config)?;
    // A managed instance is configured entirely by Elysium; locking the
    // installer keeps the first boot from exposing an unauthenticated setup UI.
    let mut env = vec![(
        "FORGEJO__security__INSTALL_LOCK".to_string(),
        EnvValue::plain("true"),
    )];
    if !config.domain.trim().is_empty() {
        env.push((
            "FORGEJO__server__DOMAIN".to_string(),
            EnvValue::plain(config.domain.trim()),
        ));
    }
    if !config.root_url.trim().is_empty() {
        env.push((
            "FORGEJO__server__ROOT_URL".to_string(),
            EnvValue::plain(config.root_url.trim()),
        ));
    }
    Ok(env)
}

/// Placeholder: the mail image contract is not pinned yet.
///
/// The mail server candidates (docs/integrations.md) configure differently
/// enough that inventing env names now would just be churn; M6 pins the image
/// and this renderer grows the real schema (hostname, domains, admin secret).
/// Until then a managed mail integration carries only the base env.
fn render_mail(config: &serde_json::Value) -> Result<RenderedEntries, TemplateError> {
    // Parsed (and rejected when malformed) even though no fields are consumed
    // yet, so a bad stored config surfaces now instead of on the M6 upgrade.
    let _: EmptyConfig = parse_config(IntegrationKind::Mail, config)?;
    Ok(Vec::new())
}

/// CNCF distribution env: `REGISTRY_<section>_<KEY>` overrides.
fn render_registry(config: &serde_json::Value) -> Result<RenderedEntries, TemplateError> {
    let _: EmptyConfig = parse_config(IntegrationKind::Registry, config)?;
    // Pin the filesystem driver's root to the `blobs` role's mount point so the
    // image and the storage-role table can never drift apart. An S3-backed
    // `blobs` binding instead needs the `REGISTRY_STORAGE_S3_*` family wired
    // from the bound Drive's config and sealed credential; that provider-native
    // wiring is owned by M5 (the registry milestone) on top of this seam.
    let blobs = storage_role(IntegrationKind::Registry, "blobs")
        .expect("the registry kind declares a blobs role");
    Ok(vec![(
        "REGISTRY_STORAGE_FILESYSTEM_ROOTDIRECTORY".to_string(),
        EnvValue::plain(blobs.mount_path),
    )])
}

/// Placeholder: the Astraval image is not published yet.
///
/// `ELYSIUM_SERVICE_TOKEN` is the intended control-plane handshake for
/// JalapenoLabs images, flowing through the same seam as the pinned kinds; the
/// Astraval milestone pins the full schema alongside the image itself.
fn render_perforce(config: &serde_json::Value) -> Result<RenderedEntries, TemplateError> {
    let _: EmptyConfig = parse_config(IntegrationKind::Perforce, config)?;
    Ok(Vec::new())
}

/// Placeholder: the runner image is not published yet (see [`render_perforce`]).
fn render_runner(config: &serde_json::Value) -> Result<RenderedEntries, TemplateError> {
    let _: EmptyConfig = parse_config(IntegrationKind::Runner, config)?;
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

    // --- render_env: the pinned kinds ----------------------------------------

    #[test]
    fn git_template_renders_the_forgejo_env() {
        let row = managed_row(
            IntegrationKind::Git,
            serde_json::json!({
                "domain": "git.example.com",
                "root_url": "https://git.example.com/",
            }),
        );
        let secret = known_secret();
        let env = render_env(&row, Some(&secret)).expect("valid config renders");

        assert_eq!(env[ENV_INTEGRATION_ID].expose(), row.id.to_string());
        assert_eq!(env["FORGEJO__security__INSTALL_LOCK"].expose(), "true");
        assert_eq!(env["FORGEJO__server__DOMAIN"].expose(), "git.example.com");
        assert_eq!(
            env["FORGEJO__server__ROOT_URL"].expose(),
            "https://git.example.com/"
        );
        // The secret lands in Forgejo's signing-key slot, tagged secret.
        let key = &env["FORGEJO__security__SECRET_KEY"];
        assert_eq!(key.expose(), secret.expose());
        assert!(key.is_secret());
        assert_eq!(env.len(), 5);
    }

    #[test]
    fn git_template_omits_unset_optional_fields() {
        // A fresh integration stores `{}`; only the always-on entries render.
        let row = managed_row(IntegrationKind::Git, serde_json::json!({}));
        let env = render_env(&row, Some(&known_secret())).expect("empty config renders");

        assert!(env.contains_key(ENV_INTEGRATION_ID));
        assert_eq!(env["FORGEJO__security__INSTALL_LOCK"].expose(), "true");
        assert!(env.contains_key("FORGEJO__security__SECRET_KEY"));
        assert!(!env.contains_key("FORGEJO__server__DOMAIN"));
        assert!(!env.contains_key("FORGEJO__server__ROOT_URL"));
    }

    #[test]
    fn registry_template_pins_the_blob_root_and_http_secret() {
        let row = managed_row(IntegrationKind::Registry, serde_json::json!({}));
        let secret = known_secret();
        let env = render_env(&row, Some(&secret)).expect("renders");

        // The filesystem driver root equals the blobs role's declared mount,
        // so the storage-role table and the image agree by construction.
        assert_eq!(
            env["REGISTRY_STORAGE_FILESYSTEM_ROOTDIRECTORY"].expose(),
            storage_role(IntegrationKind::Registry, "blobs")
                .expect("registry declares blobs")
                .mount_path
        );
        let http_secret = &env["REGISTRY_HTTP_SECRET"];
        assert_eq!(http_secret.expose(), secret.expose());
        assert!(http_secret.is_secret());
    }

    // --- render_env: the placeholder kinds -----------------------------------

    #[test]
    fn mail_template_is_the_documented_placeholder() {
        // No pinned image contract yet (M6): base env only, no secret slot.
        let row = managed_row(IntegrationKind::Mail, serde_json::json!({}));
        let env = render_env(&row, None).expect("renders");
        assert_eq!(env.len(), 1);
        assert!(env.contains_key(ENV_INTEGRATION_ID));
        assert!(!consumes_secret(IntegrationKind::Mail));
    }

    #[test]
    fn jalapenolabs_kinds_inject_the_service_token() {
        for kind in [IntegrationKind::Perforce, IntegrationKind::Runner] {
            let row = managed_row(kind, serde_json::json!({}));
            let secret = known_secret();
            let env = render_env(&row, Some(&secret)).expect("renders");

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
            let env = render_env(&row, Some(&known_secret())).expect("renders");
            assert!(env.contains_key(ENV_INTEGRATION_ID), "{kind:?} base env");
        }
    }

    // --- Malformed config ----------------------------------------------------

    #[test]
    fn malformed_config_is_a_clear_error_not_a_panic() {
        // A field of the wrong type fails the template with a message naming
        // the kind; the reconciler parks the row FAILED with it.
        let row = managed_row(IntegrationKind::Git, serde_json::json!({ "domain": 123 }));
        let error = render_env(&row, None).expect_err("bad config rejected");
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
        let env = render_env(&row, Some(&secret)).expect("renders");

        let rendered = format!("{env:?}");
        assert!(!rendered.contains(secret.expose()));
        assert!(rendered.contains("FORGEJO__security__SECRET_KEY"));
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
