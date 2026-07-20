//! The HTTP surface for linking external accounts (issue #35).
//!
//! Member endpoints ([`SessionUser`]-scoped) let a user see which providers an admin
//! configured, start a connect (which hands back the provider's authorize URL for a
//! full-page redirect), see their linked accounts, and disconnect one. The public
//! [`callback`] completes the flow: it is authenticated by the single-use `state`
//! (not a session cookie, which the provider redirect may not carry), exchanges the
//! code, links the account with sealed tokens, and 302s back to the SPA settings
//! route with `?connected=` or `?oauth_error=`. Admin endpoints
//! (`manage_integrations`) register a provider's OAuth-app credentials.
//!
//! Tokens never cross this surface: the members' reads return display fields only,
//! and the callback stores sealed tokens without ever echoing them.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::authz::{self, Capability};
use crate::oauth_client::providers::{self, Provider};
use crate::oauth_client::store::{self, UpsertAccount};
use crate::oauth_client::tokens::OauthHttp;
use crate::session::SessionUser;
use crate::state::AppState;
use crate::users;

/// The SPA route a completed connect returns to (a fixed, app-origin settings path).
///
/// Not attacker-controllable: only `?connected=`/`?oauth_error=` are appended, so a
/// callback can never be steered to redirect a code or an error off-origin.
const SETTINGS_PATH: &str = "/settings";

// --- Members: providers + connected accounts ---------------------------------

/// `GET /api/v1/oauth/providers`: the connectable providers and their status.
///
/// Lists every v1 provider with `configured = true` only when an admin has set its
/// OAuth-app credentials and enabled it. The list is fixed (github/google/microsoft);
/// the database supplies only the per-provider `configured` flag.
pub async fn list_providers(
    State(state): State<AppState>,
    SessionUser(_principal): SessionUser,
) -> Response {
    let statuses = match store::provider_statuses(&state.pool).await {
        Ok(statuses) => statuses,
        Err(error) => {
            tracing::error!(error = %error, "oauth provider status lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let providers: Vec<_> = Provider::ALL
        .into_iter()
        .map(|provider| {
            let status = statuses.iter().find(|s| s.provider == provider.slug());
            json!({
                "provider": provider.slug(),
                "display_name": provider.meta().display_name,
                "configured": store::is_configured(status),
            })
        })
        .collect();
    Json(providers).into_response()
}

/// `GET /api/v1/me/connected-accounts`: the caller's linked accounts (never tokens).
pub async fn list_connected_accounts(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    match store::list_accounts(&state.pool, user_id).await {
        Ok(accounts) => {
            let body: Vec<_> = accounts
                .into_iter()
                .map(|account| {
                    json!({
                        "id": account.id.to_string(),
                        "provider": account.provider,
                        "external_username": account.external_username,
                        "external_email": account.external_email,
                        "avatar_url": account.avatar_url,
                        "connected_at": account.connected_at.to_rfc3339(),
                        "scopes": account.scopes,
                        "needs_reconsent": account.needs_reconsent,
                    })
                })
                .collect();
            Json(body).into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "connected accounts lookup failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

/// `POST /api/v1/oauth/connect/{provider}`: begin a connect, returning the URL.
///
/// Validates the provider is configured, mints and persists a single-use `state`
/// (and a PKCE verifier where the provider supports it) bound to the caller, and
/// returns the provider's authorize URL for the SPA to redirect to. An unknown
/// provider is `404`; an unconfigured one is `400`.
pub async fn connect(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(provider): Path<String>,
) -> Response {
    let Some(provider) = Provider::from_slug(&provider) else {
        return (StatusCode::NOT_FOUND, "unknown provider").into_response();
    };
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    // The provider must be configured and enabled to be connectable.
    let config = match store::provider_config(&state.pool, &state.crypto, provider).await {
        Ok(Some(config)) if config.enabled => config,
        Ok(_) => return (StatusCode::BAD_REQUEST, "provider is not configured").into_response(),
        Err(error) => {
            tracing::error!(error = %error, "oauth provider config lookup failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    let meta = provider.meta();
    let state_value = providers::generate_state();
    // PKCE only where the provider supports it; the verifier rides the state row.
    let pkce = meta.supports_pkce.then(providers::generate_pkce);

    if let Err(error) = store::insert_state(
        &state.pool,
        &state_value,
        user_id,
        provider,
        pkce.as_ref().map(|p| p.verifier.as_str()),
        SETTINGS_PATH,
    )
    .await
    {
        tracing::error!(error = %error, "oauth state persist failed");
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }

    let redirect_uri = callback_uri(&state.app_origin, provider);
    let scopes = providers::default_scopes(meta);
    let authorize_url = providers::authorize_url(
        meta,
        &config.client_id,
        &redirect_uri,
        &scopes,
        &state_value,
        pkce.as_ref().map(|p| p.challenge.as_str()),
    );
    Json(json!({ "authorize_url": authorize_url })).into_response()
}

// --- Callback ----------------------------------------------------------------

/// The provider redirect's query parameters (both optional so we validate them).
#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    state: Option<String>,
}

/// `GET /api/v1/oauth/callback/{provider}`: complete a connect, then redirect back.
///
/// Public, authenticated by the single-use `state`: it consumes the state (binding
/// the flow to the initiating user), exchanges the code, links the account with
/// sealed tokens, and 302s to the SPA settings route with `?connected=<provider>`,
/// or `?oauth_error=<reason>` on any failure. The state is consumed exactly once, so
/// a replayed callback fails closed.
pub async fn callback(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Query(params): Query<CallbackParams>,
) -> Response {
    // Without a valid state we cannot know the intended return path, so errors
    // before the state is consumed redirect to the default settings route.
    let Some(provider) = Provider::from_slug(&provider) else {
        return oauth_error(&state.app_origin, SETTINGS_PATH, "unknown_provider");
    };
    let (Some(code), Some(state_value)) = (params.code.as_deref(), params.state.as_deref()) else {
        return oauth_error(&state.app_origin, SETTINGS_PATH, "invalid_request");
    };

    // Consume the single-use state; an unknown, expired, or reused one fails closed.
    let pending = match store::consume_state(&state.pool, state_value).await {
        Ok(Some(pending)) => pending,
        Ok(None) => return oauth_error(&state.app_origin, SETTINGS_PATH, "invalid_state"),
        Err(error) => {
            tracing::error!(error = %error, "oauth state consume failed");
            return oauth_error(&state.app_origin, SETTINGS_PATH, "server_error");
        }
    };
    let return_to = pending.redirect_after.as_str();

    // The state's provider must match the callback path (defense in depth).
    if pending.provider != provider {
        return oauth_error(&state.app_origin, return_to, "invalid_state");
    }

    let config = match store::provider_config(&state.pool, &state.crypto, provider).await {
        Ok(Some(config)) if config.enabled => config,
        Ok(_) => return oauth_error(&state.app_origin, return_to, "provider_not_configured"),
        Err(error) => {
            tracing::error!(error = %error, "oauth callback provider config lookup failed");
            return oauth_error(&state.app_origin, return_to, "server_error");
        }
    };

    let http = OauthHttp::new(state.http.clone());
    let redirect_uri = callback_uri(&state.app_origin, provider);

    // Exchange the code for tokens (presenting the PKCE verifier where one was used).
    let tokens = match http
        .exchange_code(
            provider.meta(),
            &config.client_id,
            &config.client_secret,
            &redirect_uri,
            code,
            pending.pkce_verifier.as_deref(),
        )
        .await
    {
        Ok(tokens) => tokens,
        Err(error) => {
            tracing::warn!(error = %error, provider = provider.slug(), "oauth code exchange failed");
            return oauth_error(&state.app_origin, return_to, "exchange_failed");
        }
    };

    // Fetch the external profile for the display fields and the stable external id.
    let profile = match http.fetch_profile(provider, &tokens.access_token).await {
        Ok(profile) => profile,
        Err(error) => {
            tracing::warn!(error = %error, provider = provider.slug(), "oauth profile fetch failed");
            return oauth_error(&state.app_origin, return_to, "profile_failed");
        }
    };

    let scopes = providers::parse_granted_scopes(
        tokens.scope.as_deref(),
        &providers::default_scopes(provider.meta()),
    );
    let token_expires_at = tokens.expires_at(chrono::Utc::now());

    let linked = store::upsert_account(
        &state.pool,
        &state.crypto,
        UpsertAccount {
            user_id: pending.user_id,
            provider,
            profile: &profile,
            scopes: &scopes,
            access_token: &tokens.access_token,
            refresh_token: tokens.refresh_token.as_deref(),
            token_expires_at,
        },
    )
    .await;

    match linked {
        Ok(_) => success(&state.app_origin, return_to, provider.slug()),
        Err(error) => {
            tracing::error!(error = %error, "connected account persist failed");
            oauth_error(&state.app_origin, return_to, "server_error")
        }
    }
}

/// `DELETE /api/v1/me/connected-accounts/{id}`: unlink an account, best-effort revoke.
///
/// Deletes the caller's account row (a `404` if the id is not theirs, never leaking
/// another account's existence), then fires a best-effort upstream revoke in the
/// background so the response returns immediately. Forgetting the row is the
/// authoritative revoke; the upstream call is a courtesy.
pub async fn disconnect(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(id): Path<Uuid>,
) -> Response {
    let user_id = match resolve_user(&state, &principal.sub).await {
        Ok(id) => id,
        Err(response) => return response,
    };

    let removed = match store::delete_account(&state.pool, user_id, id).await {
        Ok(Some(removed)) => removed,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "connected account delete failed");
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
    };

    spawn_best_effort_revoke(&state, removed);
    StatusCode::NO_CONTENT.into_response()
}

/// Fires a best-effort upstream token revoke in the background.
///
/// Non-fatal and off the response path: it needs the provider credentials and the
/// opened access token, and silently does nothing if either is unavailable.
fn spawn_best_effort_revoke(state: &AppState, removed: store::RemovedAccount) {
    let state = state.clone();
    tokio::spawn(async move {
        let provider = removed.provider;
        let Ok(access_token) = removed.open_access_token(&state.crypto) else {
            return;
        };
        let Ok(Some(config)) = store::provider_config(&state.pool, &state.crypto, provider).await
        else {
            return;
        };
        OauthHttp::new(state.http.clone())
            .revoke(
                provider,
                &config.client_id,
                &config.client_secret,
                &access_token,
            )
            .await;
    });
}

// --- Admin: provider configuration -------------------------------------------

/// The admin body registering a provider's OAuth-app credentials.
#[derive(Debug, Deserialize)]
pub struct ProviderConfigBody {
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub enabled: bool,
}

/// `PUT /api/v1/oauth/providers/{provider}`: set a provider's OAuth-app credentials.
///
/// Gated on `manage_integrations` in the caller's active org, matching the rule that
/// configuring integrations is an admin action. The secret is sealed at rest; setting
/// and enabling a provider is what flips it `configured` for members.
pub async fn set_provider(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(provider): Path<String>,
    Json(body): Json<ProviderConfigBody>,
) -> Response {
    let Some(provider) = Provider::from_slug(&provider) else {
        return (StatusCode::NOT_FOUND, "unknown provider").into_response();
    };
    if let Err(response) = require_manage_integrations(&state, &principal).await {
        return response;
    }

    let client_id = body.client_id.trim();
    let client_secret = body.client_secret.trim();
    if client_id.is_empty() || client_secret.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            "client_id and client_secret are required",
        )
            .into_response();
    }

    match store::upsert_provider_config(
        &state.pool,
        &state.crypto,
        provider,
        client_id,
        client_secret,
        body.enabled,
    )
    .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(error) => {
            tracing::error!(error = %error, "oauth provider config upsert failed");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

/// `GET /api/v1/oauth/providers/{provider}`: the admin echo of a provider's config.
///
/// Returns the public `client_id` and the enabled flag (never the secret), so the
/// admin console can show whether a provider is wired and prefill its id. Gated on
/// `manage_integrations`.
pub async fn get_provider(
    State(state): State<AppState>,
    SessionUser(principal): SessionUser,
    Path(provider): Path<String>,
) -> Response {
    let Some(provider) = Provider::from_slug(&provider) else {
        return (StatusCode::NOT_FOUND, "unknown provider").into_response();
    };
    if let Err(response) = require_manage_integrations(&state, &principal).await {
        return response;
    }

    match store::provider_admin_view(&state.pool, provider).await {
        Ok(view) => {
            let (client_id, enabled) = view.unwrap_or_default();
            Json(json!({
                "provider": provider.slug(),
                "display_name": provider.meta().display_name,
                "client_id": client_id,
                "enabled": enabled,
                "configured": enabled && !client_id.is_empty(),
            }))
            .into_response()
        }
        Err(error) => {
            tracing::error!(error = %error, "oauth provider admin view failed");
            StatusCode::SERVICE_UNAVAILABLE.into_response()
        }
    }
}

// --- Shared helpers ----------------------------------------------------------

/// Resolves the caller's user id, or returns a ready error response.
async fn resolve_user(state: &AppState, subject: &str) -> Result<Uuid, Response> {
    match users::id_for_subject(&state.pool, subject).await {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(StatusCode::UNAUTHORIZED.into_response()),
        Err(error) => {
            tracing::error!(error = %error, "oauth user lookup failed");
            Err(StatusCode::SERVICE_UNAVAILABLE.into_response())
        }
    }
}

/// Ensures the caller may manage integrations in their active org.
async fn require_manage_integrations(
    state: &AppState,
    principal: &crate::session::Principal,
) -> Result<(), Response> {
    let Some(org_id) = principal.active_org_id else {
        return Err((StatusCode::FORBIDDEN, "no active organization").into_response());
    };
    authz::require_capability(state, principal, org_id, Capability::ManageIntegrations).await
}

/// The OAuth redirect URI for a provider, which its OAuth app must register.
///
/// `<app_origin>/api/v1/oauth/callback/<provider>`. The SPA proxies `/api` to the
/// backend, so this is the single URL an operator registers with each provider.
fn callback_uri(app_origin: &str, provider: Provider) -> String {
    format!("{app_origin}/api/v1/oauth/callback/{}", provider.slug())
}

/// A `302` back to the SPA settings route reporting a successful connect.
fn success(app_origin: &str, path: &str, provider: &str) -> Response {
    redirect_with(app_origin, path, "connected", provider)
}

/// A `302` back to the SPA settings route reporting a failed connect.
fn oauth_error(app_origin: &str, path: &str, reason: &str) -> Response {
    redirect_with(app_origin, path, "oauth_error", reason)
}

/// Builds the settings redirect with a single appended query parameter.
fn redirect_with(app_origin: &str, path: &str, key: &str, value: &str) -> Response {
    let separator = if path.contains('?') { '&' } else { '?' };
    let location = format!("{app_origin}{path}{separator}{key}={}", encode(value));
    match header::HeaderValue::from_str(&location) {
        Ok(value) => (StatusCode::FOUND, [(header::LOCATION, value)]).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

/// Percent-encodes a query value, preserving only the RFC 3986 unreserved set.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char);
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_uri_is_the_app_origin_api_path() {
        assert_eq!(
            callback_uri("http://localhost:5173", Provider::Github),
            "http://localhost:5173/api/v1/oauth/callback/github"
        );
    }

    #[test]
    fn success_and_error_redirect_to_the_settings_route() {
        let ok = success("http://localhost:5173", "/settings", "github");
        assert_eq!(ok.status(), StatusCode::FOUND);
        assert_eq!(
            ok.headers().get(header::LOCATION).unwrap(),
            "http://localhost:5173/settings?connected=github"
        );

        let err = oauth_error("http://localhost:5173", "/settings", "invalid_state");
        assert_eq!(
            err.headers().get(header::LOCATION).unwrap(),
            "http://localhost:5173/settings?oauth_error=invalid_state"
        );
    }

    #[test]
    fn redirect_appends_with_the_right_separator() {
        // A path that already carries a query uses '&'.
        let response = success("https://app.test", "/settings?tab=1", "google");
        assert_eq!(
            response.headers().get(header::LOCATION).unwrap(),
            "https://app.test/settings?tab=1&connected=google"
        );
    }
}
