//! The provider HTTP calls, the silent-refresh decision, and the access-token seam.
//!
//! [`OauthHttp`] wraps the shared rustls `reqwest` client (no OpenSSL, matching the
//! rest of the tree) and makes the three outbound calls a connect needs: exchange a
//! code for tokens, fetch the external profile, and later refresh an expiring grant.
//! The token exchange is factored behind the private [`RefreshEndpoint`] seam so the
//! refresh logic is unit-testable with a mock, never a real network call.
//!
//! [`resolve_connected_token`] is the seam the app and Elysia call to get a valid
//! access token for a linked account: it returns the current token, silently
//! refreshing an expiring one via the refresh token, and flags `needs_reconsent`
//! (returning `None`) only when a refresh genuinely fails. The refresh/needs-reconsent
//! decision is the pure [`decide_refresh`], so it is testable without a clock or I/O.

use std::future::Future;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::crypto::Crypto;
use crate::oauth_client::providers::{self, ExternalProfile, Provider, ProviderMeta};
use crate::oauth_client::store::{self, ProviderConfig};

/// How long to wait on a provider before giving up (token and profile calls).
const HTTP_TIMEOUT: StdDuration = StdDuration::from_secs(10);

/// The `User-Agent` sent to providers; GitHub rejects API calls without one.
const USER_AGENT: &str = "Elysium";

/// Refresh this long before the actual expiry, so a token cannot lapse mid-request.
const REFRESH_SKEW: Duration = Duration::seconds(60);

/// A provider token-endpoint response, normalized across providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenResponse {
    pub access_token: String,
    /// A refresh token, when the provider issued one (many GitHub grants have none).
    pub refresh_token: Option<String>,
    /// Seconds until the access token expires, when the provider reports it.
    pub expires_in: Option<i64>,
    /// The space/comma-delimited granted scopes, when the provider echoes them.
    pub scope: Option<String>,
}

impl TokenResponse {
    /// The absolute expiry of this response's access token relative to `now`.
    pub fn expires_at(&self, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
        self.expires_in
            .map(|seconds| now + Duration::seconds(seconds))
    }
}

/// The raw token-endpoint body, tolerating the provider `error` shape.
#[derive(Debug, Deserialize)]
struct RawToken {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    scope: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

/// Interprets a decoded token body as success or a provider error.
///
/// GitHub answers a failed exchange with HTTP 200 and an `error` field, so success
/// is defined by the presence of an `access_token`, not the status code.
///
/// # Errors
/// Returns an error carrying the provider's `error`/`error_description` when no
/// access token is present.
fn interpret_token(raw: RawToken) -> eyre::Result<TokenResponse> {
    match raw.access_token {
        Some(access_token) => Ok(TokenResponse {
            access_token,
            refresh_token: raw.refresh_token,
            expires_in: raw.expires_in,
            scope: raw.scope,
        }),
        None => {
            let code = raw.error.unwrap_or_else(|| "unknown_error".to_string());
            let detail = raw.error_description.unwrap_or_default();
            Err(eyre::eyre!(
                "token endpoint rejected the request: {code} {detail}"
            ))
        }
    }
}

// --- The outbound HTTP client ------------------------------------------------

/// Makes the provider HTTP calls a connect and refresh need.
///
/// Cloneable (it holds a `reqwest::Client` handle, itself connection-pooled), so it
/// is built from the shared [`crate::state::AppState::http`] client and passed by value.
#[derive(Clone)]
pub struct OauthHttp {
    client: reqwest::Client,
}

impl OauthHttp {
    /// Wraps an existing shared `reqwest` client (the app's pooled client).
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    /// Exchanges an authorization `code` for tokens (presenting PKCE when used).
    ///
    /// # Errors
    /// Returns an error on a transport failure or a provider error response.
    pub async fn exchange_code(
        &self,
        meta: &ProviderMeta,
        client_id: &str,
        client_secret: &str,
        redirect_uri: &str,
        code: &str,
        code_verifier: Option<&str>,
    ) -> eyre::Result<TokenResponse> {
        let mut form: Vec<(&str, &str)> = vec![
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect_uri),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];
        if let Some(verifier) = code_verifier {
            form.push(("code_verifier", verifier));
        }
        self.post_token(meta, &form).await
    }

    /// Fetches and normalizes the external profile for an access token.
    ///
    /// For GitHub, the primary verified address is resolved from `/user/emails` when
    /// `/user` withholds the email; other providers carry it in the userinfo body.
    ///
    /// # Errors
    /// Returns an error on a transport failure or when the profile lacks the stable
    /// external id (an unexpected response shape).
    pub async fn fetch_profile(
        &self,
        provider: Provider,
        access_token: &str,
    ) -> eyre::Result<ExternalProfile> {
        let meta = provider.meta();
        let userinfo: Value = self
            .client
            .get(meta.userinfo_url)
            .bearer_auth(access_token)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(HTTP_TIMEOUT)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        // GitHub only: pull the emails list to fill a withheld address. Best-effort;
        // a failure here just leaves the email blank rather than failing the link.
        let emails: Option<Value> = match meta.emails_url {
            Some(url) => self.fetch_emails(url, access_token).await,
            None => None,
        };

        providers::parse_profile(provider, &userinfo, emails.as_ref())
            .ok_or_else(|| eyre::eyre!("provider profile is missing the external id"))
    }

    /// Fetches a provider's emails list (GitHub's `/user/emails`), best-effort.
    ///
    /// Returns `None` on any transport, status, or decode error; the caller treats a
    /// missing list as simply "no email to backfill", never a hard failure.
    async fn fetch_emails(&self, url: &str, access_token: &str) -> Option<Value> {
        let response = self
            .client
            .get(url)
            .bearer_auth(access_token)
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .header(reqwest::header::ACCEPT, "application/json")
            .timeout(HTTP_TIMEOUT)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        response.json().await.ok()
    }

    /// Best-effort upstream revoke; failures are swallowed by design.
    ///
    /// Revocation is a courtesy to the provider: Elysium forgets the row regardless,
    /// so a provider that is unreachable or lacks a revoke endpoint never blocks a
    /// disconnect. GitHub and Google expose a revoke; Microsoft has no simple one.
    pub async fn revoke(
        &self,
        provider: Provider,
        client_id: &str,
        client_secret: &str,
        access_token: &str,
    ) {
        let attempt = match provider {
            Provider::Github => {
                self.client
                    .delete(format!(
                        "https://api.github.com/applications/{client_id}/token"
                    ))
                    .basic_auth(client_id, Some(client_secret))
                    .header(reqwest::header::USER_AGENT, USER_AGENT)
                    .json(&serde_json::json!({ "access_token": access_token }))
                    .timeout(HTTP_TIMEOUT)
                    .send()
                    .await
            }
            Provider::Google => {
                self.client
                    .post("https://oauth2.googleapis.com/revoke")
                    .form(&[("token", access_token)])
                    .timeout(HTTP_TIMEOUT)
                    .send()
                    .await
            }
            // No standard, simple revoke; forgetting the row is the effective revoke.
            Provider::Microsoft => return,
        };
        if let Err(error) = attempt {
            tracing::debug!(error = %error, provider = provider.slug(), "best-effort oauth revoke failed");
        }
    }

    /// POSTs a form to the provider token endpoint and interprets the response.
    async fn post_token(
        &self,
        meta: &ProviderMeta,
        form: &[(&str, &str)],
    ) -> eyre::Result<TokenResponse> {
        let raw: RawToken = self
            .client
            .post(meta.token_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::USER_AGENT, USER_AGENT)
            .form(form)
            .timeout(HTTP_TIMEOUT)
            .send()
            .await?
            .json()
            .await?;
        interpret_token(raw)
    }
}

// --- The refresh endpoint seam (mockable) ------------------------------------

/// The single token-endpoint call the refresh logic depends on, factored out so a
/// test can supply a mock provider instead of making a real network call.
trait RefreshEndpoint {
    fn refresh(
        &self,
        meta: &ProviderMeta,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> impl Future<Output = eyre::Result<TokenResponse>> + Send;
}

impl RefreshEndpoint for OauthHttp {
    async fn refresh(
        &self,
        meta: &ProviderMeta,
        client_id: &str,
        client_secret: &str,
        refresh_token: &str,
    ) -> eyre::Result<TokenResponse> {
        let form: Vec<(&str, &str)> = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", client_id),
            ("client_secret", client_secret),
        ];
        self.post_token(meta, &form).await
    }
}

// --- The silent-refresh decision (pure) --------------------------------------

/// What to do with a stored access token when a caller asks for a valid one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RefreshDecision {
    /// The current access token is still fresh; use it as-is.
    Valid,
    /// The token is expiring; renew it with the refresh token.
    Refresh,
    /// The token is expiring and cannot be renewed; re-consent is required.
    Reconsent,
}

/// Decides whether a stored token is still usable, renewable, or dead.
///
/// A token with no known expiry is always [`RefreshDecision::Valid`] (GitHub
/// OAuth-app tokens do not expire). One within [`REFRESH_SKEW`] of expiry is
/// [`RefreshDecision::Refresh`] when a refresh token exists, else
/// [`RefreshDecision::Reconsent`]. `now` is explicit so the logic is clock-free.
fn decide_refresh(
    expires_at: Option<DateTime<Utc>>,
    has_refresh_token: bool,
    now: DateTime<Utc>,
) -> RefreshDecision {
    match expires_at {
        None => RefreshDecision::Valid,
        Some(expiry) if expiry > now + REFRESH_SKEW => RefreshDecision::Valid,
        Some(_) if has_refresh_token => RefreshDecision::Refresh,
        Some(_) => RefreshDecision::Reconsent,
    }
}

/// The inputs to a resolution, already unsealed and loaded from the database.
struct ResolveInput<'a> {
    access_token: String,
    refresh_token: Option<String>,
    token_expires_at: Option<DateTime<Utc>>,
    config: Option<&'a ProviderConfig>,
}

/// The outcome of resolving a token, to be applied to the database by the caller.
#[derive(Debug, PartialEq, Eq)]
enum Resolution {
    /// The stored token is valid; return it unchanged.
    Use(String),
    /// A refresh succeeded; persist these tokens and return the new access token.
    Refreshed {
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    },
    /// The grant cannot be renewed; flag it for re-consent.
    Reconsent,
}

/// Computes a resolution from already-loaded inputs, refreshing via `endpoint`.
///
/// This is the DB-free core of [`resolve_connected_token`]: it makes at most one
/// call, to `endpoint.refresh`, so a test drives every branch with a mock. A refresh
/// is attempted only when the token is expiring, a refresh token and app credentials
/// both exist, and it downgrades to [`Resolution::Reconsent`] the moment any of that
/// is missing or the refresh call fails.
async fn compute_resolution<E: RefreshEndpoint>(
    meta: &ProviderMeta,
    input: ResolveInput<'_>,
    endpoint: &E,
    now: DateTime<Utc>,
) -> Resolution {
    match decide_refresh(input.token_expires_at, input.refresh_token.is_some(), now) {
        RefreshDecision::Valid => Resolution::Use(input.access_token),
        RefreshDecision::Reconsent => Resolution::Reconsent,
        RefreshDecision::Refresh => {
            // A refresh needs both the refresh token and the app credentials.
            let (Some(refresh_token), Some(config)) = (input.refresh_token, input.config) else {
                return Resolution::Reconsent;
            };
            match endpoint
                .refresh(
                    meta,
                    &config.client_id,
                    &config.client_secret,
                    &refresh_token,
                )
                .await
            {
                Ok(response) => Resolution::Refreshed {
                    access_token: response.access_token.clone(),
                    refresh_token: response.refresh_token.clone(),
                    expires_at: response.expires_at(now),
                },
                // A genuinely failed refresh surfaces as a single re-consent prompt.
                Err(error) => {
                    tracing::warn!(error = %error, "oauth token refresh failed; re-consent required");
                    Resolution::Reconsent
                }
            }
        }
    }
}

/// Returns a valid access token for a linked account, refreshing silently as needed.
///
/// The seam the integration surfaces (and Elysia) call: it opens the stored token,
/// returns it when fresh, transparently refreshes an expiring one and re-seals the
/// rotated tokens, and sets `needs_reconsent` (returning `None`) only when a refresh
/// genuinely fails or the grant already needs re-consent. `None` therefore means
/// exactly "the user must re-connect this account".
///
/// # Errors
/// Returns an error only when the database or a sealed token cannot be read; a failed
/// refresh is not an error, it is a `None` with `needs_reconsent` set.
// The access-token resolution seam lands ahead of its consumers (the git/mail/registry
// verticals). Mirrors `transport.rs`'s "seam ahead of consumers" allowance.
#[allow(
    dead_code,
    reason = "linked-integration access-token seam; per-service verticals wire the consumer"
)]
pub async fn resolve_connected_token(
    pool: &PgPool,
    crypto: &Crypto,
    connected_account_id: Uuid,
) -> eyre::Result<Option<String>> {
    let Some(grant) = store::load_grant(pool, connected_account_id).await? else {
        return Ok(None);
    };
    // Already flagged: do not retry a dead grant, surface re-consent to the caller.
    if grant.needs_reconsent {
        return Ok(None);
    }

    let config = store::provider_config(pool, crypto, grant.provider).await?;
    let input = ResolveInput {
        access_token: grant.open_access_token(crypto)?,
        refresh_token: grant.open_refresh_token(crypto)?,
        token_expires_at: grant.token_expires_at,
        config: config.as_ref(),
    };

    let endpoint = OauthHttp::new(reqwest::Client::new());
    match compute_resolution(grant.provider.meta(), input, &endpoint, Utc::now()).await {
        Resolution::Use(access_token) => Ok(Some(access_token)),
        Resolution::Refreshed {
            access_token,
            refresh_token,
            expires_at,
        } => {
            store::store_refreshed(
                pool,
                crypto,
                &grant,
                &access_token,
                refresh_token.as_deref(),
                expires_at,
            )
            .await?;
            Ok(Some(access_token))
        }
        Resolution::Reconsent => {
            store::mark_needs_reconsent(pool, grant.id).await?;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid instant")
    }

    fn config() -> ProviderConfig {
        ProviderConfig {
            client_id: "client".to_string(),
            client_secret: "secret".to_string(),
            enabled: true,
        }
    }

    // --- interpret_token -----------------------------------------------------

    #[test]
    fn interpret_token_reads_success_and_surfaces_provider_errors() {
        let ok = interpret_token(RawToken {
            access_token: Some("at".to_string()),
            refresh_token: Some("rt".to_string()),
            expires_in: Some(3600),
            scope: Some("openid email".to_string()),
            error: None,
            error_description: None,
        })
        .expect("a body with an access token is success");
        assert_eq!(ok.access_token, "at");
        assert_eq!(ok.refresh_token.as_deref(), Some("rt"));
        assert_eq!(ok.expires_at(now()), Some(now() + Duration::seconds(3600)));

        // GitHub returns 200 with an error field; no access token is a failure.
        let err = interpret_token(RawToken {
            access_token: None,
            refresh_token: None,
            expires_in: None,
            scope: None,
            error: Some("bad_verification_code".to_string()),
            error_description: Some("The code is incorrect.".to_string()),
        });
        assert!(err.is_err());
    }

    // --- decide_refresh (pure) -----------------------------------------------

    #[test]
    fn a_token_without_expiry_is_always_valid() {
        // GitHub OAuth-app tokens never expire, so they are used as-is.
        assert_eq!(decide_refresh(None, false, now()), RefreshDecision::Valid);
    }

    #[test]
    fn a_token_far_from_expiry_is_valid() {
        let expiry = now() + Duration::hours(1);
        assert_eq!(
            decide_refresh(Some(expiry), true, now()),
            RefreshDecision::Valid
        );
    }

    #[test]
    fn a_near_expiry_token_triggers_refresh_within_the_skew() {
        // Inside the skew window (expires in 30s): renew rather than risk a lapse.
        let expiry = now() + Duration::seconds(30);
        assert_eq!(
            decide_refresh(Some(expiry), true, now()),
            RefreshDecision::Refresh
        );
        // Already expired, with a refresh token: refresh.
        let expired = now() - Duration::seconds(1);
        assert_eq!(
            decide_refresh(Some(expired), true, now()),
            RefreshDecision::Refresh
        );
    }

    #[test]
    fn a_near_expiry_token_without_a_refresh_token_needs_reconsent() {
        let expiry = now() + Duration::seconds(10);
        assert_eq!(
            decide_refresh(Some(expiry), false, now()),
            RefreshDecision::Reconsent
        );
    }

    // --- compute_resolution, with a mocked token endpoint --------------------

    /// A mock refresh endpoint: returns a preset response, and records the call.
    struct MockRefresh {
        response: std::sync::Mutex<Option<eyre::Result<TokenResponse>>>,
        calls: std::sync::atomic::AtomicUsize,
    }

    impl MockRefresh {
        fn ok(response: TokenResponse) -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Ok(response))),
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn failing() -> Self {
            Self {
                response: std::sync::Mutex::new(Some(Err(eyre::eyre!("provider said no")))),
                calls: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn calls(&self) -> usize {
            self.calls.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl RefreshEndpoint for MockRefresh {
        async fn refresh(
            &self,
            _meta: &ProviderMeta,
            _client_id: &str,
            _client_secret: &str,
            _refresh_token: &str,
        ) -> eyre::Result<TokenResponse> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.response
                .lock()
                .expect("lock")
                .take()
                .expect("the mock is called at most once per test")
        }
    }

    fn meta() -> &'static ProviderMeta {
        Provider::Google.meta()
    }

    #[tokio::test]
    async fn a_fresh_token_is_used_without_calling_the_endpoint() {
        let mock = MockRefresh::failing(); // must not be called
        let input = ResolveInput {
            access_token: "still-good".to_string(),
            refresh_token: Some("rt".to_string()),
            token_expires_at: Some(now() + Duration::hours(1)),
            config: None,
        };
        let outcome = compute_resolution(meta(), input, &mock, now()).await;
        assert_eq!(outcome, Resolution::Use("still-good".to_string()));
        assert_eq!(mock.calls(), 0, "a fresh token needs no refresh call");
    }

    #[tokio::test]
    async fn an_expiring_token_is_refreshed() {
        let mock = MockRefresh::ok(TokenResponse {
            access_token: "new-access".to_string(),
            refresh_token: Some("new-refresh".to_string()),
            expires_in: Some(3600),
            scope: None,
        });
        let config = config();
        let input = ResolveInput {
            access_token: "expiring".to_string(),
            refresh_token: Some("old-refresh".to_string()),
            token_expires_at: Some(now() - Duration::seconds(1)),
            config: Some(&config),
        };
        let outcome = compute_resolution(meta(), input, &mock, now()).await;
        assert_eq!(
            outcome,
            Resolution::Refreshed {
                access_token: "new-access".to_string(),
                refresh_token: Some("new-refresh".to_string()),
                expires_at: Some(now() + Duration::seconds(3600)),
            }
        );
        assert_eq!(mock.calls(), 1);
    }

    #[tokio::test]
    async fn a_failed_refresh_surfaces_reconsent() {
        let mock = MockRefresh::failing();
        let config = config();
        let input = ResolveInput {
            access_token: "expiring".to_string(),
            refresh_token: Some("old-refresh".to_string()),
            token_expires_at: Some(now() - Duration::seconds(1)),
            config: Some(&config),
        };
        let outcome = compute_resolution(meta(), input, &mock, now()).await;
        assert_eq!(outcome, Resolution::Reconsent);
        assert_eq!(
            mock.calls(),
            1,
            "the refresh was attempted before giving up"
        );
    }

    #[tokio::test]
    async fn an_expiring_token_without_credentials_cannot_refresh() {
        let mock = MockRefresh::failing(); // must not be called: no config
        let input = ResolveInput {
            access_token: "expiring".to_string(),
            refresh_token: Some("old-refresh".to_string()),
            token_expires_at: Some(now() - Duration::seconds(1)),
            config: None,
        };
        let outcome = compute_resolution(meta(), input, &mock, now()).await;
        assert_eq!(outcome, Resolution::Reconsent);
        assert_eq!(mock.calls(), 0);
    }
}
