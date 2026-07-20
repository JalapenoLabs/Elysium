//! Transport credentials: the SSH keys and scoped access tokens an account uses
//! to authenticate transport (git over SSH/HTTPS, `docker login`, SMTP AUTH).
//!
//! These extend the Security Center (see [`crate::security`]) but are a different
//! shape of credential from a passkey or TOTP: transport clients present a key or
//! bearer token, not an OIDC assertion, so Elysium **mints** them and later
//! **resolves** a presented one back to its owning account. See
//! `docs/integrations.md` (Identity, transport auth).
//!
//! Two invariants define the module:
//!
//! - **Secrets are shown once.** A server-generated SSH private key and a minted
//!   token are returned exactly once at creation and never retrievable again.
//!   Storage keeps only the SSH public half and its fingerprint, or a one-way hash
//!   of the token, so a database dump reveals no usable credential.
//! - **Revocation is immediate.** Deleting a key or token removes the row, and the
//!   resolution path ([`resolve_ssh_key_owner`], [`resolve_access_token`]) checks
//!   live state, so transport auth fails the instant a credential is revoked.
//!
//! Tokens are hashed, not sealed with [`crate::crypto`], on purpose: sealing is for
//! secrets Elysium must recover in plaintext (TOTP secrets, Drive credentials); a
//! bearer token is only ever *compared*, so a one-way hash is both sufficient and
//! strictly safer, a dump never yields tokens even with the master key.

use std::fmt::{self, Write as _};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use rand::rngs::OsRng;
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey};
use uuid::Uuid;

use crate::orchestration::integrations::{self, IntegrationRow};
use crate::proto::common::v1::Timestamp;
use crate::proto::security::v1 as pb;
use crate::protobuf::Protobuf;
use crate::session::SessionUser;
use crate::state::AppState;
use crate::users;

/// Prefix on every minted token's plaintext.
///
/// Makes a leaked token greppable and detectable by secret scanners; it is not
/// secret, only the high-entropy tail after it is. Chosen to read as "Elysium
/// personal access token", mirroring the `ghp_`/`github_pat_` convention.
const TOKEN_PREFIX: &str = "ely_pat_";

/// CSPRNG entropy in a minted token, in bytes (256 bits).
///
/// Large enough that a token is unguessable and its SHA-256 hash is collision-free
/// in practice; the acceptance criteria require at least 32 bytes.
const TOKEN_ENTROPY_BYTES: usize = 32;

// --- Scope mapping -----------------------------------------------------------

/// Maps a wire [`pb::TokenScope`] to its stored smallint, rejecting UNSPECIFIED.
///
/// The proto discriminants are the persisted values, so the mapping is the
/// identity cast, except the zero `UNSPECIFIED` sentinel, which never persists: a
/// token with no capability is meaningless.
fn scope_to_db(scope: pb::TokenScope) -> Option<i16> {
    match scope {
        pb::TokenScope::Unspecified => None,
        // Every real scope's discriminant fits a smallint and equals its DB value.
        other => Some(other as i16),
    }
}

/// Parses a stored scope smallint back to the wire enum; unknown or zero is `None`.
fn scope_from_db(value: i16) -> Option<pb::TokenScope> {
    match pb::TokenScope::try_from(i32::from(value)) {
        Ok(pb::TokenScope::Unspecified) | Err(_) => None,
        Ok(scope) => Some(scope),
    }
}

/// Validates a mint request's scopes into a deduplicated set of stored smallints.
///
/// Rejects the whole request (as a user-facing message the handler maps to `400`)
/// when the set is empty or carries the `UNSPECIFIED`/unknown sentinel: a token
/// must grant at least one real capability.
///
/// # Errors
/// Returns the rejection message when the scope set is empty or malformed.
fn parse_scopes(raw: &[i32]) -> Result<Vec<i16>, &'static str> {
    if raw.is_empty() {
        return Err("at least one scope is required");
    }
    let mut scopes = Vec::with_capacity(raw.len());
    for value in raw {
        let scope = pb::TokenScope::try_from(*value)
            .ok()
            .and_then(scope_to_db)
            .ok_or("a scope is unspecified or unknown")?;
        if !scopes.contains(&scope) {
            scopes.push(scope);
        }
    }
    Ok(scopes)
}

// --- SSH key materialization -------------------------------------------------

/// A materialized SSH key: the public half to store, plus the private half to
/// return once when Elysium generated the pair.
///
/// The fingerprint and algorithm are always derived from the key itself, never
/// trusted from client input. `private_key` is present only on the generate path;
/// its custom [`fmt::Debug`] keeps the private material out of logs.
struct MaterializedKey {
    /// The OpenSSH public key line, e.g. `ssh-ed25519 AAAA... work-laptop`.
    public_key: String,
    /// The SHA256 fingerprint, e.g. `SHA256:abc…`, derived from the public key.
    fingerprint: String,
    /// The key algorithm string, e.g. `ssh-ed25519`.
    algorithm: String,
    /// The OpenSSH-format private key, present only when Elysium generated the pair.
    private_key: Option<String>,
}

// Redacts the private half: a stray log line must never carry key material, so
// Debug prints a fixed marker instead (see the `debug_redacts_private_key` test).
impl fmt::Debug for MaterializedKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MaterializedKey")
            .field("fingerprint", &self.fingerprint)
            .field("algorithm", &self.algorithm)
            .field(
                "private_key",
                &self.private_key.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

/// Describes the parts of a [`PublicKey`] Elysium stores, deriving them itself.
fn describe_public_key(
    public: &PublicKey,
    private_key: Option<String>,
) -> ssh_key::Result<MaterializedKey> {
    let algorithm = public.algorithm();
    Ok(MaterializedKey {
        public_key: public.to_openssh()?,
        fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
        algorithm: algorithm.as_str().to_string(),
        private_key,
    })
}

/// Generates an ed25519 keypair, labeling the public key with `comment`.
///
/// Returns the public half to store and the OpenSSH-format private half to return
/// exactly once. The private key is never persisted; it is the user's to keep.
///
/// # Errors
/// Returns an error only if the CSPRNG-backed key encoding fails, which does not
/// happen for an in-memory ed25519 key.
fn generate_ed25519(comment: &str) -> ssh_key::Result<MaterializedKey> {
    let mut private = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)?;
    private.set_comment(sanitize_comment(comment));
    let private_pem = private.to_openssh(LineEnding::LF)?.to_string();
    describe_public_key(private.public_key(), Some(private_pem))
}

/// Parses and validates a user-supplied OpenSSH public key line.
///
/// Derives the fingerprint and algorithm from the parsed key, never trusting
/// client-provided values, and never returns a private key (the user keeps it).
///
/// # Errors
/// Returns an error when the input is not a valid OpenSSH public key, which the
/// handler maps to `400`.
fn parse_public_key(line: &str) -> ssh_key::Result<MaterializedKey> {
    let public = PublicKey::from_openssh(line.trim())?;
    describe_public_key(&public, None)
}

/// Strips control characters from an SSH key comment so it cannot corrupt the
/// single-line OpenSSH public key encoding.
fn sanitize_comment(comment: &str) -> String {
    comment
        .trim()
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

// --- Token secrets -----------------------------------------------------------

/// Mints a fresh bearer token secret: `ely_pat_` followed by 256 CSPRNG bits.
///
/// The entropy is URL-safe base64 (unpadded) so the whole token is a clean,
/// copy-pasteable string. Shown to the user exactly once; only its hash persists.
fn generate_token_secret() -> String {
    let mut entropy = [0u8; TOKEN_ENTROPY_BYTES];
    OsRng.fill_bytes(&mut entropy);
    format!("{TOKEN_PREFIX}{}", URL_SAFE_NO_PAD.encode(entropy))
}

/// The lookup hash of a token secret: lowercase-hex SHA-256 of the plaintext.
///
/// A bearer token is only ever compared, so we store this one-way hash rather than
/// a recoverable seal. Verification hashes the presented token and looks the hash
/// up; the same input always yields the same hash, so the lookup is exact.
fn hash_token(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Infallible: writing to a String never fails.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

// --- Timestamp helper --------------------------------------------------------

/// Builds a UTC protobuf [`Timestamp`] from a `chrono` instant; the zone is display-only.
fn to_timestamp(instant: DateTime<Utc>) -> Timestamp {
    Timestamp {
        epoch_seconds: instant.timestamp(),
        nanos: instant.timestamp_subsec_nanos(),
        timezone: "Etc/UTC".to_string(),
    }
}

// --- Shared handler helpers --------------------------------------------------

/// Resolves the caller's user id, or returns a ready error response.
async fn resolve_user(state: &AppState, subject: &str) -> Result<Uuid, Response> {
    match users::id_for_subject(&state.pool, subject).await {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "transport-credential user lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Maps a scoped delete to 204 (done) or 404 (nothing the caller owns matched).
///
/// A `404` for an id the caller does not own, rather than `403`, so the endpoint
/// never leaks the existence of another account's credential.
fn respond_deleted(
    result: sqlx::Result<sqlx::postgres::PgQueryResult>,
    operation: &str,
) -> Response {
    match result {
        Ok(done) if done.rows_affected() > 0 => StatusCode::NO_CONTENT.into_response(),
        Ok(_) => StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, operation, "transport-credential delete failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

// --- SSH key endpoints -------------------------------------------------------

/// A stored `ssh_keys` row, as returned to the account owner (never a private key).
#[derive(sqlx::FromRow)]
struct SshKeyRow {
    id: Uuid,
    name: String,
    fingerprint: String,
    public_key: String,
    algorithm: String,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl SshKeyRow {
    /// Builds the protobuf [`pb::SshKey`]; the private half is never part of it.
    fn to_proto(&self) -> pb::SshKey {
        pb::SshKey {
            id: self.id.to_string(),
            name: self.name.clone(),
            fingerprint: self.fingerprint.clone(),
            public_key: self.public_key.clone(),
            algorithm: self.algorithm.clone(),
            created_at: Some(to_timestamp(self.created_at)),
            last_used_at: self.last_used_at.map(to_timestamp),
        }
    }
}

/// `GET /api/v1/me/ssh-keys`: the caller's registered SSH keys, newest first.
///
/// Returns only public material: each key's label, SHA256 fingerprint, OpenSSH
/// public line, and algorithm. A private key is never stored, so it is never here.
pub async fn list_ssh_keys(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let rows = sqlx::query_as::<_, SshKeyRow>(
        "SELECT id, name, fingerprint, public_key, algorithm, created_at, last_used_at
           FROM ssh_keys WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Protobuf(pb::SshKeyList {
            keys: rows.iter().map(SshKeyRow::to_proto).collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "ssh key list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `POST /api/v1/me/ssh-keys`: register a public key, or generate a keypair.
///
/// An empty `public_key` asks Elysium to generate an ed25519 keypair and return
/// the private half exactly once; a non-empty one is parsed and validated as an
/// OpenSSH public key (malformed input is a `400`). Either way the fingerprint is
/// derived server-side. A key already registered to the account is a `409`.
pub async fn register_ssh_key(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<pb::RegisterSshKeyRequest>,
) -> Response {
    let name = request.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }

    // Empty public_key means "generate the pair for me"; otherwise validate the
    // supplied one. Both paths derive the fingerprint from the key, not the client.
    let materialized = if request.public_key.trim().is_empty() {
        match generate_ed25519(name) {
            Ok(key) => key,
            Err(error) => {
                tracing::error!(error = %error, "ssh keypair generation failed");
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
        }
    } else {
        match parse_public_key(&request.public_key) {
            Ok(key) => key,
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    "public_key is not a valid OpenSSH public key",
                )
                    .into_response()
            }
        }
    };

    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let inserted = sqlx::query_as::<_, SshKeyRow>(
        "INSERT INTO ssh_keys (user_id, name, fingerprint, public_key, algorithm)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, name, fingerprint, public_key, algorithm, created_at, last_used_at",
    )
    .bind(user_id)
    .bind(name)
    .bind(&materialized.fingerprint)
    .bind(&materialized.public_key)
    .bind(&materialized.algorithm)
    .fetch_one(&state.pool)
    .await;

    match inserted {
        Ok(row) => Protobuf(pb::SshKeyCreated {
            key: Some(row.to_proto()),
            // Populated only on the generate path; carried once, never stored.
            private_key: materialized.private_key.unwrap_or_default(),
        })
        .into_response(),
        Err(error) if is_unique_violation(&error) => {
            (StatusCode::CONFLICT, "this key is already registered").into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "ssh key registration failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/me/ssh-keys/{id}`: revoke one of the caller's SSH keys.
///
/// The row is deleted, so [`resolve_ssh_key_owner`] stops resolving it at once:
/// transport auth checks live state, making revocation immediate.
pub async fn delete_ssh_key(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    let result = sqlx::query("DELETE FROM ssh_keys WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&state.pool)
        .await;
    respond_deleted(result, "ssh_key.delete")
}

// --- Access token endpoints --------------------------------------------------

/// A stored `access_tokens` row, as returned to the account owner (never a secret).
#[derive(sqlx::FromRow)]
struct AccessTokenRow {
    id: Uuid,
    name: String,
    scopes: Vec<i16>,
    integration_id: Option<Uuid>,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    last_used_at: Option<DateTime<Utc>>,
}

impl AccessTokenRow {
    /// Builds the protobuf [`pb::AccessToken`]; the secret is never part of it.
    ///
    /// A stored scope smallint outside the known set is dropped rather than
    /// surfaced, so a future scope this build predates cannot corrupt the response.
    fn to_proto(&self) -> pb::AccessToken {
        pb::AccessToken {
            id: self.id.to_string(),
            name: self.name.clone(),
            scopes: self
                .scopes
                .iter()
                .filter_map(|value| scope_from_db(*value))
                .map(|scope| scope as i32)
                .collect(),
            integration_id: self
                .integration_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            expires_at: self.expires_at.map(to_timestamp),
            created_at: Some(to_timestamp(self.created_at)),
            last_used_at: self.last_used_at.map(to_timestamp),
        }
    }
}

/// `GET /api/v1/me/tokens`: the caller's access tokens, newest first.
///
/// Returns each token's label, scopes, optional integration bind, and timestamps.
/// The secret is never stored, so it is never here.
pub async fn list_tokens(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let rows = sqlx::query_as::<_, AccessTokenRow>(
        "SELECT id, name, scopes, integration_id, expires_at, created_at, last_used_at
           FROM access_tokens WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => Protobuf(pb::AccessTokenList {
            tokens: rows.iter().map(AccessTokenRow::to_proto).collect(),
        })
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "token list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `POST /api/v1/me/tokens`: mint a scoped bearer token, shown once.
///
/// Validates the scope set (non-empty, no `UNSPECIFIED`), an optional
/// `integration_id` the caller must be able to access, and the TTL (`0` means no
/// expiry). Stores only the token's SHA-256 hash; the plaintext `ely_pat_...`
/// secret is returned in this response and never again.
pub async fn mint_token(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Protobuf(request): Protobuf<pb::MintTokenRequest>,
) -> Response {
    let name = request.name.trim();
    if name.is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    let scopes = match parse_scopes(&request.scopes) {
        Ok(scopes) => scopes,
        Err(message) => return (StatusCode::BAD_REQUEST, message).into_response(),
    };

    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    // An optional bind to one integration instance the caller can access. An empty
    // string leaves the token unscoped to a specific instance.
    let integration_id =
        match validate_integration_bind(&state, user_id, &request.integration_id).await {
            Ok(id) => id,
            Err(response) => return response,
        };

    // ttl_seconds == 0 means "never expires" (NULL); otherwise now() + ttl.
    let expires_at: Option<DateTime<Utc>> = if request.ttl_seconds > 0 {
        Some(Utc::now() + chrono::Duration::seconds(request.ttl_seconds))
    } else {
        None
    };

    let secret = generate_token_secret();
    let token_hash = hash_token(&secret);

    let inserted = sqlx::query_as::<_, AccessTokenRow>(
        "INSERT INTO access_tokens (user_id, name, token_hash, scopes, integration_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)
         RETURNING id, name, scopes, integration_id, expires_at, created_at, last_used_at",
    )
    .bind(user_id)
    .bind(name)
    .bind(&token_hash)
    .bind(&scopes)
    .bind(integration_id)
    .bind(expires_at)
    .fetch_one(&state.pool)
    .await;

    match inserted {
        Ok(row) => Protobuf(pb::AccessTokenMinted {
            token: Some(row.to_proto()),
            // The plaintext bearer value, carried once here; only its hash persists.
            secret,
        })
        .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "token mint failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `DELETE /api/v1/me/tokens/{id}`: revoke one of the caller's tokens.
///
/// The row is deleted, so [`resolve_access_token`] stops resolving it at once:
/// verification checks live state, making revocation immediate.
pub async fn revoke_token(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };
    let result = sqlx::query("DELETE FROM access_tokens WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(user_id)
        .execute(&state.pool)
        .await;
    respond_deleted(result, "access_token.delete")
}

/// Validates an optional `integration_id` string on a mint request.
///
/// Empty is `Ok(None)` (the token is not bound to a specific instance). Otherwise
/// the id must parse and reference an integration the caller can access, or the
/// bind is rejected with a `400` that does not distinguish "unknown" from
/// "inaccessible", so a caller cannot probe another tenant's integrations.
///
/// # Errors
/// Returns a ready `Response` (`400` for a bad or inaccessible id, `503` on a
/// database error).
async fn validate_integration_bind(
    state: &AppState,
    user_id: Uuid,
    integration_id: &str,
) -> Result<Option<Uuid>, Response> {
    let trimmed = integration_id.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let Ok(integration_id) = Uuid::parse_str(trimmed) else {
        return Err((StatusCode::BAD_REQUEST, "integration_id is not a valid id").into_response());
    };

    let row = match sqlx::query_as::<_, IntegrationRow>("SELECT * FROM integrations WHERE id = $1")
        .bind(integration_id)
        .fetch_optional(&state.pool)
        .await
    {
        Ok(row) => row,
        Err(error) => {
            tracing::error!(error = %error, "token integration bind lookup failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    let Some(row) = row else {
        return Err((
            StatusCode::BAD_REQUEST,
            "unknown or inaccessible integration",
        )
            .into_response());
    };

    let mut conn = match state.pool.acquire().await {
        Ok(conn) => conn,
        Err(error) => {
            tracing::error!(error = %error, "token integration bind connection acquire failed");
            return Err(StatusCode::SERVICE_UNAVAILABLE.into_response());
        }
    };
    match integrations::can_access_integration(&mut conn, user_id, &row).await {
        Ok(true) => Ok(Some(integration_id)),
        Ok(false) => Err((
            StatusCode::BAD_REQUEST,
            "unknown or inaccessible integration",
        )
            .into_response()),
        Err(error) => {
            tracing::error!(error = %error, "token integration bind access resolution failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Whether a database error is a unique-constraint violation (`23505`).
fn is_unique_violation(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .and_then(|db_error| db_error.code())
        .as_deref()
        == Some("23505")
}

// --- Transport-auth resolution (#29 / #38) -----------------------------------

/// Resolves a presented SSH public key to the account that owns it.
///
/// The transport-auth path (SSH routing, #29 / #38) calls this with the OpenSSH
/// public key line an inbound connection presented. It derives the SHA256
/// fingerprint from the key itself and looks the owner up in the registry.
/// Returns the owning user id, or `None` when the key is unparseable or
/// unregistered; a revoked key is gone from the table, so resolution fails closed
/// the instant it is deleted.
///
/// # Errors
/// Returns an error only if the database query fails, never for an unknown key.
// The transport-auth resolution seam lands ahead of its consumers; #29/#38 wire
// the SSH router that calls it. Mirrors `orchestration`'s "seam ahead of
// consumers" allowance rather than `expect`, since the DB-gated tests below use
// it, which would leave an `expect(dead_code)` unfulfilled under `--all-targets`.
#[allow(
    dead_code,
    reason = "transport-auth resolution seam; #29/#38 wire the consumer"
)]
pub async fn resolve_ssh_key_owner(pool: &PgPool, public_key: &str) -> sqlx::Result<Option<Uuid>> {
    let Ok(parsed) = PublicKey::from_openssh(public_key.trim()) else {
        return Ok(None);
    };
    let fingerprint = parsed.fingerprint(HashAlg::Sha256).to_string();
    sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM ssh_keys WHERE fingerprint = $1 ORDER BY created_at ASC LIMIT 1",
    )
    .bind(fingerprint)
    .fetch_optional(pool)
    .await
}

/// A successfully authenticated bearer token: its owner and what it may do.
///
/// Returned by [`resolve_access_token`] so the transport-auth path can enforce the
/// token's capability (its `scopes`) and its optional instance bind
/// (`integration_id`) before allowing a git, registry, or SMTP operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "transport-auth resolution seam; #29/#38 wire the consumer"
)]
pub struct TokenGrant {
    pub token_id: Uuid,
    pub user_id: Uuid,
    pub scopes: Vec<pb::TokenScope>,
    pub integration_id: Option<Uuid>,
}

/// Authenticates a presented bearer token, returning its grant when valid.
///
/// The transport-auth path (HTTPS git, `docker login`, SMTP AUTH; #29 / #38) calls
/// this with the token a client presented. It hashes the token and looks the hash
/// up, honoring expiry, so an expired, revoked (deleted), or unknown token resolves
/// to `None` and fails closed. Usage stamping (`last_used_at`) is the caller's to
/// wire; this issue leaves it `NULL`.
///
/// # Errors
/// Returns an error only if the database query fails, never for an invalid token.
#[allow(
    dead_code,
    reason = "transport-auth resolution seam; #29/#38 wire the consumer"
)]
pub async fn resolve_access_token(pool: &PgPool, secret: &str) -> sqlx::Result<Option<TokenGrant>> {
    let token_hash = hash_token(secret);
    let row = sqlx::query_as::<_, AccessTokenRow>(
        "SELECT id, name, scopes, integration_id, expires_at, created_at, last_used_at
           FROM access_tokens
          WHERE token_hash = $1 AND (expires_at IS NULL OR expires_at > now())",
    )
    .bind(token_hash)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };
    // Resolve the owner separately: the row select above intentionally excludes
    // user_id from the owner-facing projection, so read it directly here.
    let user_id = sqlx::query_scalar::<_, Uuid>("SELECT user_id FROM access_tokens WHERE id = $1")
        .bind(row.id)
        .fetch_one(pool)
        .await?;

    Ok(Some(TokenGrant {
        token_id: row.id,
        user_id,
        scopes: row
            .scopes
            .iter()
            .filter_map(|value| scope_from_db(*value))
            .collect(),
        integration_id: row.integration_id,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Scope mapping -------------------------------------------------------

    /// Every real scope, for exhaustive round-trip checks.
    const REAL_SCOPES: [pb::TokenScope; 5] = [
        pb::TokenScope::GitRead,
        pb::TokenScope::GitWrite,
        pb::TokenScope::RegistryPull,
        pb::TokenScope::RegistryPush,
        pb::TokenScope::MailSend,
    ];

    #[test]
    fn scope_round_trips_through_the_database_value() {
        for scope in REAL_SCOPES {
            let db = scope_to_db(scope).expect("a real scope maps to a smallint");
            // The stored smallint is the proto discriminant, and it maps back.
            assert_eq!(i32::from(db), scope as i32);
            assert_eq!(scope_from_db(db), Some(scope));
        }
    }

    #[test]
    fn scope_mapping_rejects_the_unspecified_sentinel() {
        assert_eq!(scope_to_db(pb::TokenScope::Unspecified), None);
        // The zero value never round-trips back out of the database either.
        assert_eq!(scope_from_db(0), None);
        // An unknown discriminant is dropped, not surfaced as a bogus scope.
        assert_eq!(scope_from_db(99), None);
    }

    #[test]
    fn parse_scopes_requires_at_least_one_real_scope() {
        assert!(parse_scopes(&[]).is_err());
        assert!(parse_scopes(&[pb::TokenScope::Unspecified as i32]).is_err());
        // A single unknown discriminant is rejected, not silently dropped.
        assert!(parse_scopes(&[9999]).is_err());
    }

    #[test]
    fn parse_scopes_deduplicates_while_preserving_the_stored_values() {
        let raw = [
            pb::TokenScope::GitRead as i32,
            pb::TokenScope::GitRead as i32,
            pb::TokenScope::GitWrite as i32,
        ];
        let scopes = parse_scopes(&raw).expect("valid scopes parse");
        assert_eq!(
            scopes,
            vec![
                pb::TokenScope::GitRead as i16,
                pb::TokenScope::GitWrite as i16,
            ]
        );
    }

    // --- Token secrets -------------------------------------------------------

    #[test]
    fn generated_token_has_the_prefix_and_enough_entropy() {
        let secret = generate_token_secret();
        assert!(secret.starts_with(TOKEN_PREFIX));
        let entropy = secret.strip_prefix(TOKEN_PREFIX).expect("prefix present");
        let decoded = URL_SAFE_NO_PAD
            .decode(entropy)
            .expect("the tail is base64url");
        assert!(
            decoded.len() >= 32,
            "a token must carry at least 256 bits of entropy, got {} bytes",
            decoded.len()
        );
    }

    #[test]
    fn each_token_secret_is_unique() {
        assert_ne!(generate_token_secret(), generate_token_secret());
    }

    #[test]
    fn token_hash_is_deterministic_so_lookup_matches() {
        let secret = "ely_pat_example";
        // The same secret always hashes the same way: this is what makes the
        // hash-then-look-up verification path resolve a presented token.
        assert_eq!(hash_token(secret), hash_token(secret));
        // A different secret hashes differently, so tokens do not collide.
        assert_ne!(hash_token(secret), hash_token("ely_pat_other"));
        // Lowercase-hex SHA-256 is 64 characters.
        assert_eq!(hash_token(secret).len(), 64);
        assert!(hash_token(secret).chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn token_hash_matches_a_known_sha256_vector() {
        // SHA-256("abc") is a published test vector; proves we hash the plaintext.
        assert_eq!(
            hash_token("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    // --- SSH key generation and registration ---------------------------------

    #[test]
    fn generated_keypair_is_parseable_with_a_matching_fingerprint() {
        let key = generate_ed25519("work laptop").expect("generation succeeds");

        // The public line parses back, and re-deriving its fingerprint matches the
        // one we stored: the fingerprint is computed from the key, not asserted.
        let parsed = PublicKey::from_openssh(&key.public_key).expect("public line parses");
        assert_eq!(
            parsed.fingerprint(HashAlg::Sha256).to_string(),
            key.fingerprint
        );

        assert!(key.fingerprint.starts_with("SHA256:"));
        assert_eq!(key.algorithm, "ssh-ed25519");

        // The private half is returned once, in OpenSSH PEM form.
        let private = key.private_key.expect("generate returns a private key");
        assert!(private.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----"));
        // And it truly pairs with the stored public key.
        let reparsed = PrivateKey::from_openssh(&private).expect("private key parses");
        assert_eq!(
            reparsed
                .public_key()
                .fingerprint(HashAlg::Sha256)
                .to_string(),
            key.fingerprint
        );
    }

    #[test]
    fn registering_a_valid_public_key_derives_the_same_fingerprint() {
        // Generate a key, then register only its public line, as a user would.
        let generated = generate_ed25519("laptop").expect("generation succeeds");
        let registered = parse_public_key(&generated.public_key).expect("valid public key");

        assert_eq!(registered.fingerprint, generated.fingerprint);
        assert_eq!(registered.algorithm, "ssh-ed25519");
        // Registration never yields a private key; the user keeps their own.
        assert!(registered.private_key.is_none());
    }

    #[test]
    fn registering_malformed_input_is_rejected() {
        assert!(parse_public_key("not a key").is_err());
        assert!(parse_public_key("ssh-ed25519 not-base64!!").is_err());
        assert!(parse_public_key("").is_err());
    }

    #[test]
    fn parse_public_key_derives_algorithm_and_fingerprint_ignoring_the_comment() {
        // Two registrations of the same key with different comments resolve to the
        // same fingerprint: the comment is cosmetic, the key material identifies it.
        let generated = generate_ed25519("first").expect("generation succeeds");
        let public = &generated.public_key;
        let base = public
            .rsplit_once(' ')
            .map_or(public.as_str(), |(head, _)| head);
        let with_other_comment = format!("{base} someone@example.com");

        let a = parse_public_key(public).expect("valid");
        let b = parse_public_key(&with_other_comment).expect("valid");
        assert_eq!(a.fingerprint, b.fingerprint);
    }

    // --- Redaction -----------------------------------------------------------

    #[test]
    fn debug_redacts_the_private_key() {
        let key = generate_ed25519("secret laptop").expect("generation succeeds");
        let private = key.private_key.clone().expect("has a private key");
        let rendered = format!("{key:?}");

        // The private material must never appear in a debug rendering.
        assert!(!rendered.contains(&private));
        assert!(rendered.contains("<redacted>"));
        // Non-secret fields remain visible for debugging.
        assert!(rendered.contains(&key.fingerprint));
    }

    // --- Row -> proto never leaks secrets ------------------------------------

    #[test]
    fn access_token_proto_drops_unknown_stored_scopes() {
        let row = AccessTokenRow {
            id: Uuid::new_v4(),
            name: "ci".to_string(),
            // A valid scope alongside a stored value this build does not know.
            scopes: vec![pb::TokenScope::GitRead as i16, 42],
            integration_id: None,
            expires_at: None,
            created_at: Utc::now(),
            last_used_at: None,
        };
        let proto = row.to_proto();
        assert_eq!(proto.scopes, vec![pb::TokenScope::GitRead as i32]);
        // An absent integration bind renders as the empty string, per the contract.
        assert_eq!(proto.integration_id, "");
    }

    // --- DB-gated resolution and revocation ----------------------------------
    //
    // These need a reachable postgres; they are ignored by default and run in CI
    // via `cargo test -- --ignored` (see tests/migrations.rs for the convention).

    /// Connects to the test database and brings the schema up to date.
    #[cfg(test)]
    async fn connect_test_pool() -> PgPool {
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

    /// Inserts a throwaway user, returning its id, for resolution tests.
    ///
    /// Both credential tables reference `users` alone, so no org membership is
    /// needed; identity is multi-org (see migration 0010), keyed by subject/email.
    #[cfg(test)]
    async fn seed_user(pool: &PgPool) -> Uuid {
        let suffix = Uuid::new_v4();
        sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("subject-{suffix}"))
        .bind(format!("user-{suffix}@example.test"))
        .fetch_one(pool)
        .await
        .expect("seed user")
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn ssh_key_resolves_to_its_owner_then_stops_on_revocation() {
        let pool = connect_test_pool().await;
        let user_id = seed_user(&pool).await;
        let key = generate_ed25519("laptop").expect("generation succeeds");

        let key_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO ssh_keys (user_id, name, fingerprint, public_key, algorithm)
             VALUES ($1, 'laptop', $2, $3, $4) RETURNING id",
        )
        .bind(user_id)
        .bind(&key.fingerprint)
        .bind(&key.public_key)
        .bind(&key.algorithm)
        .fetch_one(&pool)
        .await
        .expect("insert key");

        // A presented public key resolves to the registering account.
        let resolved = resolve_ssh_key_owner(&pool, &key.public_key)
            .await
            .expect("resolution query succeeds");
        assert_eq!(resolved, Some(user_id));

        // Revocation is immediate: once deleted, the same key resolves to nobody.
        sqlx::query("DELETE FROM ssh_keys WHERE id = $1")
            .bind(key_id)
            .execute(&pool)
            .await
            .expect("delete key");
        let after = resolve_ssh_key_owner(&pool, &key.public_key)
            .await
            .expect("resolution query succeeds");
        assert_eq!(after, None);
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn access_token_resolves_by_hash_and_honors_expiry_and_revocation() {
        let pool = connect_test_pool().await;
        let user_id = seed_user(&pool).await;
        let secret = generate_token_secret();

        let token_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO access_tokens (user_id, name, token_hash, scopes)
             VALUES ($1, 'ci', $2, $3) RETURNING id",
        )
        .bind(user_id)
        .bind(hash_token(&secret))
        .bind(vec![
            pb::TokenScope::GitRead as i16,
            pb::TokenScope::GitWrite as i16,
        ])
        .fetch_one(&pool)
        .await
        .expect("insert token");

        // The presented secret hashes to the stored hash and resolves the grant.
        let grant = resolve_access_token(&pool, &secret)
            .await
            .expect("resolution query succeeds")
            .expect("a live token resolves");
        assert_eq!(grant.token_id, token_id);
        assert_eq!(grant.user_id, user_id);
        assert_eq!(
            grant.scopes,
            vec![pb::TokenScope::GitRead, pb::TokenScope::GitWrite]
        );

        // A wrong secret never resolves, even against a live row.
        assert!(resolve_access_token(&pool, "ely_pat_wrong")
            .await
            .expect("resolution query succeeds")
            .is_none());

        // An expired token fails closed without being deleted.
        sqlx::query(
            "UPDATE access_tokens SET expires_at = now() - interval '1 hour' WHERE id = $1",
        )
        .bind(token_id)
        .execute(&pool)
        .await
        .expect("expire token");
        assert!(resolve_access_token(&pool, &secret)
            .await
            .expect("resolution query succeeds")
            .is_none());

        // Revocation is immediate: a deleted token resolves to nobody.
        sqlx::query("UPDATE access_tokens SET expires_at = NULL WHERE id = $1")
            .bind(token_id)
            .execute(&pool)
            .await
            .expect("un-expire token");
        sqlx::query("DELETE FROM access_tokens WHERE id = $1")
            .bind(token_id)
            .execute(&pool)
            .await
            .expect("delete token");
        assert!(resolve_access_token(&pool, &secret)
            .await
            .expect("resolution query succeeds")
            .is_none());
    }
}
