//! The OIDC client registry: registered relying parties and their authentication.
//!
//! A client is a service that SSOs against Elysium. Managed services (Forgejo,
//! Harbor, ...) are registered here at provisioning time (#38) with a sealed
//! secret injected into their container config; the [`create_client`] and
//! [`get_client`] seams exist for that caller. A minimal admin-gated
//! register/list endpoint lets an operator register a client by hand meanwhile.
//!
//! Client secrets are sealed with [`crate::crypto`], not hashed, on purpose: the
//! managed service needs the plaintext injected into its own config, so Elysium
//! must recover it. This is the opposite trade-off from a bearer token (hashed,
//! [`crate::transport`]), which Elysium only ever compares. A confidential client
//! proves itself with its secret (constant-time compared); a public client proves
//! itself with PKCE alone and carries no secret.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::PgPool;
use uuid::Uuid;

use crate::authz::{self, Capability};
use crate::crypto::Crypto;
use crate::oidc::{constant_time_eq, SUPPORTED_GRANT_TYPES, SUPPORTED_SCOPES};
use crate::session::SessionUser;
use crate::state::AppState;

/// Prefix on a generated confidential client secret, mirroring the `ely_pat_`
/// transport-token convention: greppable, scanner-detectable, not itself secret.
const CLIENT_SECRET_PREFIX: &str = "ely_cs_";

/// CSPRNG entropy in a generated client secret, in bytes (256 bits).
const CLIENT_SECRET_ENTROPY_BYTES: usize = 32;

/// Parameters for registering a client, shared by the endpoint and #38.
#[derive(Debug, Clone)]
pub struct CreateClientParams {
    /// The desired public client id; a fresh one is generated when `None`.
    pub client_id: Option<String>,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub grant_types: Vec<String>,
    /// Confidential clients get a sealed secret; public (PKCE-only) clients do not.
    pub confidential: bool,
    /// First-party (managed) clients auto-approve with no consent screen.
    pub is_first_party: bool,
    /// The managed integration this client fronts, when provisioned for one.
    pub integration_id: Option<Uuid>,
}

/// A newly registered client; the secret is returned once and never again.
#[derive(Debug, Clone)]
pub struct CreatedClient {
    pub client_id: String,
    /// The plaintext secret for a confidential client, carried once; `None` for a
    /// public client.
    pub client_secret: Option<String>,
}

/// A registered client as loaded for the authorize/token flow.
///
/// `sealed_secret` is the at-rest ciphertext; it never leaves this crate and is
/// only opened inside [`authenticate`]. `is_confidential` is derived from its
/// presence.
#[derive(Debug, Clone)]
pub struct Client {
    pub client_id: String,
    pub name: String,
    pub redirect_uris: Vec<String>,
    pub scopes: Vec<String>,
    pub grant_types: Vec<String>,
    pub is_first_party: bool,
    sealed_secret: Option<Vec<u8>>,
}

impl Client {
    /// Whether this client authenticates with a secret (vs. a public PKCE client).
    pub fn is_confidential(&self) -> bool {
        self.sealed_secret.is_some()
    }

    /// Whether `uri` exactly matches one of the client's registered redirect URIs.
    ///
    /// Exact match only, never a prefix: a code is never returned to a URI the
    /// client did not register, which is what makes the redirect a safe channel.
    pub fn allows_redirect(&self, uri: &str) -> bool {
        self.redirect_uris.iter().any(|allowed| allowed == uri)
    }

    /// Whether the client is permitted to use `grant_type`.
    pub fn allows_grant(&self, grant_type: &str) -> bool {
        self.grant_types.iter().any(|allowed| allowed == grant_type)
    }
}

/// The stored `oidc_clients` row.
#[derive(sqlx::FromRow)]
struct ClientRow {
    client_id: String,
    client_secret: Option<Vec<u8>>,
    name: String,
    redirect_uris: Vec<String>,
    scopes: Vec<String>,
    grant_types: Vec<String>,
    is_first_party: bool,
}

impl From<ClientRow> for Client {
    fn from(row: ClientRow) -> Self {
        Self {
            client_id: row.client_id,
            name: row.name,
            redirect_uris: row.redirect_uris,
            scopes: row.scopes,
            grant_types: row.grant_types,
            is_first_party: row.is_first_party,
            sealed_secret: row.client_secret,
        }
    }
}

/// Generates a confidential client secret: `ely_cs_` plus 256 CSPRNG bits.
fn generate_client_secret() -> String {
    let mut entropy = [0u8; CLIENT_SECRET_ENTROPY_BYTES];
    OsRng.fill_bytes(&mut entropy);
    format!("{CLIENT_SECRET_PREFIX}{}", URL_SAFE_NO_PAD.encode(entropy))
}

/// Registers a client, sealing its secret when confidential.
///
/// The provisioning path (#38) calls this to register a managed service, then
/// injects the returned `client_secret` into the service's config (#31). A blank
/// `client_id` is filled with a fresh `ely_client_<random>` value.
///
/// # Errors
/// Returns an error if the insert fails (including a duplicate `client_id`).
pub async fn create_client(
    pool: &PgPool,
    crypto: &Crypto,
    params: CreateClientParams,
) -> eyre::Result<CreatedClient> {
    let client_id = params.client_id.unwrap_or_else(|| {
        let mut entropy = [0u8; 16];
        OsRng.fill_bytes(&mut entropy);
        format!("ely_client_{}", URL_SAFE_NO_PAD.encode(entropy))
    });

    // Mint and seal the secret only for confidential clients; the plaintext is
    // returned once and the sealed form (bound to the client id) is stored.
    let (plaintext, sealed) = if params.confidential {
        let secret = generate_client_secret();
        let sealed = crypto.seal(secret.as_bytes(), client_id.as_bytes());
        (Some(secret), Some(sealed))
    } else {
        (None, None)
    };

    sqlx::query(
        "INSERT INTO oidc_clients
            (client_id, client_secret, name, redirect_uris, scopes, grant_types,
             integration_id, is_first_party)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
    )
    .bind(&client_id)
    .bind(&sealed)
    .bind(&params.name)
    .bind(&params.redirect_uris)
    .bind(&params.scopes)
    .bind(&params.grant_types)
    .bind(params.integration_id)
    .bind(params.is_first_party)
    .execute(pool)
    .await
    .map_err(|error| eyre::eyre!("failed to register OIDC client: {error}"))?;

    Ok(CreatedClient {
        client_id,
        client_secret: plaintext,
    })
}

/// Loads a client by its public id, or `None` when unregistered.
///
/// # Errors
/// Returns an error if the database is unreachable.
pub async fn get_client(pool: &PgPool, client_id: &str) -> sqlx::Result<Option<Client>> {
    let row = sqlx::query_as::<_, ClientRow>(
        "SELECT client_id, client_secret, name, redirect_uris, scopes, grant_types, is_first_party
           FROM oidc_clients WHERE client_id = $1",
    )
    .bind(client_id)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(Client::from))
}

/// Authenticates a token-endpoint client from its presented secret.
///
/// A confidential client must present the secret that unseals to its stored one
/// (constant-time compared, so a wrong secret leaks no timing signal). A public
/// client presents none and is accepted on PKCE alone. Returns `true` when the
/// client authenticated.
///
/// # Errors
/// Returns an error only if opening the sealed secret fails (wrong master key or
/// tampering), never for a wrong presented secret.
pub fn authenticate(
    client: &Client,
    crypto: &Crypto,
    presented_secret: Option<&str>,
) -> eyre::Result<bool> {
    match &client.sealed_secret {
        // Confidential: require the secret and compare it in constant time.
        Some(sealed) => {
            let Some(presented) = presented_secret else {
                return Ok(false);
            };
            let opened = crypto
                .open(sealed, client.client_id.as_bytes())
                .map_err(|error| eyre::eyre!("failed to open a client secret: {error}"))?;
            Ok(constant_time_eq(presented.as_bytes(), &opened.plaintext))
        }
        // Public: PKCE-only, no secret is expected or accepted.
        None => Ok(true),
    }
}

// --- Admin register/list endpoint --------------------------------------------

/// The JSON body for registering a client by hand.
#[derive(Debug, Deserialize)]
pub struct RegisterClientBody {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub redirect_uris: Vec<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
    #[serde(default)]
    pub grant_types: Vec<String>,
    /// Defaults to a confidential client; set false for a public PKCE-only client.
    #[serde(default = "default_true")]
    pub confidential: bool,
    #[serde(default)]
    pub is_first_party: bool,
}

fn default_true() -> bool {
    true
}

/// `POST /api/v1/oidc/clients`: register a client (needs `manage_integrations`).
///
/// Gated on the caller's active org, matching the rule that provisioning
/// integrations (which a managed OIDC client fronts) is `manage_integrations`. The
/// response carries the one-time client secret for a confidential client.
pub async fn register(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Json(body): Json<RegisterClientBody>,
) -> Response {
    let Some(org_id) = principal.active_org_id else {
        return (StatusCode::FORBIDDEN, "no active organization").into_response();
    };
    if let Err(response) =
        authz::require_capability(&state, &principal, org_id, Capability::ManageIntegrations).await
    {
        return response;
    }

    if body.name.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "name is required").into_response();
    }
    if body.redirect_uris.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "at least one redirect_uri is required",
        )
            .into_response();
    }

    // Default to the standard scope/grant sets when the caller omits them, and
    // reject anything the provider does not support so a client cannot be
    // registered to request a scope the token endpoint will never honor.
    let scopes = if body.scopes.is_empty() {
        vec![
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
        ]
    } else {
        body.scopes
    };
    if let Some(bad) = scopes
        .iter()
        .find(|scope| !SUPPORTED_SCOPES.contains(&scope.as_str()))
    {
        return (StatusCode::BAD_REQUEST, format!("unsupported scope: {bad}")).into_response();
    }
    let grant_types = if body.grant_types.is_empty() {
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ]
    } else {
        body.grant_types
    };
    if let Some(bad) = grant_types
        .iter()
        .find(|grant| !SUPPORTED_GRANT_TYPES.contains(&grant.as_str()))
    {
        return (
            StatusCode::BAD_REQUEST,
            format!("unsupported grant_type: {bad}"),
        )
            .into_response();
    }

    let params = CreateClientParams {
        client_id: None,
        name: body.name.trim().to_string(),
        redirect_uris: body.redirect_uris,
        scopes,
        grant_types,
        confidential: body.confidential,
        is_first_party: body.is_first_party,
        integration_id: None,
    };

    match create_client(&state.pool, &state.crypto, params).await {
        Ok(created) => (
            StatusCode::CREATED,
            Json(json!({
                "client_id": created.client_id,
                // Carried once; a confidential client's secret is never retrievable again.
                "client_secret": created.client_secret,
            })),
        )
            .into_response(),
        Err(error) => {
            tracing::error!(error = %error, "OIDC client registration failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/v1/oidc/clients`: list registered clients (needs `manage_integrations`).
///
/// Returns each client's public metadata; a secret is never included.
pub async fn list(State(state): State<AppState>, SessionUser(principal): SessionUser) -> Response {
    let Some(org_id) = principal.active_org_id else {
        return (StatusCode::FORBIDDEN, "no active organization").into_response();
    };
    if let Err(response) =
        authz::require_capability(&state, &principal, org_id, Capability::ManageIntegrations).await
    {
        return response;
    }

    let rows = sqlx::query_as::<_, ClientRow>(
        "SELECT client_id, client_secret, name, redirect_uris, scopes, grant_types, is_first_party
           FROM oidc_clients ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await;

    match rows {
        Ok(rows) => {
            let clients: Vec<Value> = rows
                .into_iter()
                .map(Client::from)
                .map(|client| {
                    json!({
                        "client_id": client.client_id,
                        "name": client.name,
                        "redirect_uris": client.redirect_uris,
                        "scopes": client.scopes,
                        "grant_types": client.grant_types,
                        "is_first_party": client.is_first_party,
                        "is_confidential": client.is_confidential(),
                    })
                })
                .collect();
            Json(json!({ "clients": clients })).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "OIDC client list failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_crypto() -> Crypto {
        Crypto::new(&[9u8; 32]).expect("32-byte key is valid")
    }

    /// Builds a client with a sealed secret, as the registry would store it.
    fn confidential_client(crypto: &Crypto, client_id: &str, secret: &str) -> Client {
        Client {
            client_id: client_id.to_string(),
            name: "test".to_string(),
            redirect_uris: vec!["https://svc.example/callback".to_string()],
            scopes: vec!["openid".to_string()],
            grant_types: vec!["authorization_code".to_string()],
            is_first_party: true,
            sealed_secret: Some(crypto.seal(secret.as_bytes(), client_id.as_bytes())),
        }
    }

    #[test]
    fn confidential_client_authenticates_only_with_the_right_secret() {
        let crypto = test_crypto();
        let client = confidential_client(&crypto, "svc", "ely_cs_correct");

        assert!(authenticate(&client, &crypto, Some("ely_cs_correct")).expect("opens"));
        // A wrong secret is refused, and a confidential client with no secret fails.
        assert!(!authenticate(&client, &crypto, Some("ely_cs_wrong")).expect("opens"));
        assert!(!authenticate(&client, &crypto, None).expect("opens"));
    }

    #[test]
    fn public_client_authenticates_with_no_secret() {
        let crypto = test_crypto();
        let client = Client {
            client_id: "spa".to_string(),
            name: "spa".to_string(),
            redirect_uris: vec!["https://spa.example/cb".to_string()],
            scopes: vec!["openid".to_string()],
            grant_types: vec!["authorization_code".to_string()],
            is_first_party: false,
            sealed_secret: None,
        };
        assert!(!client.is_confidential());
        // PKCE-only: authentication succeeds without a secret.
        assert!(authenticate(&client, &crypto, None).expect("no secret to open"));
    }

    #[test]
    fn redirect_uri_matching_is_exact() {
        let crypto = test_crypto();
        let client = confidential_client(&crypto, "svc", "s");
        assert!(client.allows_redirect("https://svc.example/callback"));
        // A different path, a prefix, or an appended query never matches.
        assert!(!client.allows_redirect("https://svc.example/callback/evil"));
        assert!(!client.allows_redirect("https://svc.example"));
        assert!(!client.allows_redirect("https://svc.example/callback?x=1"));
    }

    #[test]
    fn generated_client_secret_has_the_prefix_and_entropy() {
        let secret = generate_client_secret();
        assert!(secret.starts_with(CLIENT_SECRET_PREFIX));
        let tail = secret.strip_prefix(CLIENT_SECRET_PREFIX).expect("prefix");
        assert_eq!(
            URL_SAFE_NO_PAD.decode(tail).expect("base64url").len(),
            CLIENT_SECRET_ENTROPY_BYTES
        );
        assert_ne!(generate_client_secret(), generate_client_secret());
    }
}
