//! Database access for the OAuth-client subsystem, with tokens sealed at rest.
//!
//! Three tables (migration 0029): [`oauth_provider_configs`] holds the admin app
//! credentials (secret sealed), [`oauth_states`] holds short-lived single-use
//! connect state, and [`connected_accounts`] holds the linked accounts with their
//! sealed access/refresh tokens. Every stored token is sealed with [`crate::crypto`]
//! bound to the connected account's stable identity (`user:provider:external_id`),
//! so a database dump cannot open a token, nor shuffle one between accounts or users.
//!
//! Access and refresh tokens are sealed, not hashed (the transport-token trade-off
//! inverts here): Elysium must present the plaintext token to the provider on the
//! user's behalf, so it must be recoverable, unlike a bearer token it only compares.

use chrono::{DateTime, Duration, Utc};
use sqlx::PgPool;
use uuid::Uuid;

use crate::crypto::Crypto;
use crate::oauth_client::providers::{self, ExternalProfile, Provider};

/// The admin OAuth-app credentials for a provider, with the secret opened.
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub client_id: String,
    /// The opened (plaintext) client secret; never logged or returned to a client.
    pub client_secret: String,
    pub enabled: bool,
}

/// A provider's configuration status, for the members' providers list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub provider: String,
    pub enabled: bool,
}

/// Whether a provider is offered to members: it has credentials **and** is enabled.
///
/// A missing row or a configured-but-disabled provider is not `configured`, so it
/// cannot be connected. This is the single derivation the providers list uses.
pub fn is_configured(status: Option<&ProviderStatus>) -> bool {
    status.is_some_and(|status| status.enabled)
}

// --- Provider configs --------------------------------------------------------

/// Lists each configured provider's slug and enabled flag (no secrets opened).
///
/// # Errors
/// Returns an error if the query fails.
pub async fn provider_statuses(pool: &PgPool) -> sqlx::Result<Vec<ProviderStatus>> {
    let rows =
        sqlx::query_as::<_, (String, bool)>("SELECT provider, enabled FROM oauth_provider_configs")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(provider, enabled)| ProviderStatus { provider, enabled })
        .collect())
}

/// Loads and opens a provider's app credentials, or `None` when unconfigured.
///
/// # Errors
/// Returns an error if the query fails or the sealed secret cannot be opened (wrong
/// master key or tampering).
pub async fn provider_config(
    pool: &PgPool,
    crypto: &Crypto,
    provider: Provider,
) -> eyre::Result<Option<ProviderConfig>> {
    let row = sqlx::query_as::<_, (String, Vec<u8>, bool)>(
        "SELECT client_id, client_secret, enabled FROM oauth_provider_configs WHERE provider = $1",
    )
    .bind(provider.slug())
    .fetch_optional(pool)
    .await?;

    let Some((client_id, sealed, enabled)) = row else {
        return Ok(None);
    };
    let opened = crypto
        .open(&sealed, provider.slug().as_bytes())
        .map_err(|error| eyre::eyre!("failed to open a provider client secret: {error}"))?;
    Ok(Some(ProviderConfig {
        client_id,
        client_secret: String::from_utf8(opened.plaintext)
            .map_err(|_| eyre::eyre!("a provider client secret is not valid UTF-8"))?,
        enabled,
    }))
}

/// The admin echo of a provider's config: its public `client_id` and enabled flag.
///
/// Never opens the sealed secret; the admin UI uses this only to prefill the id.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn provider_admin_view(
    pool: &PgPool,
    provider: Provider,
) -> sqlx::Result<Option<(String, bool)>> {
    sqlx::query_as::<_, (String, bool)>(
        "SELECT client_id, enabled FROM oauth_provider_configs WHERE provider = $1",
    )
    .bind(provider.slug())
    .fetch_optional(pool)
    .await
}

/// Stores (or replaces) a provider's app credentials, sealing the secret.
///
/// The secret is sealed bound to the provider slug (so it cannot be replayed under
/// another provider), mirroring how an OIDC client secret binds to its client id.
///
/// # Errors
/// Returns an error if the upsert fails.
pub async fn upsert_provider_config(
    pool: &PgPool,
    crypto: &Crypto,
    provider: Provider,
    client_id: &str,
    client_secret: &str,
    enabled: bool,
) -> sqlx::Result<()> {
    let sealed = crypto.seal(client_secret.as_bytes(), provider.slug().as_bytes());
    sqlx::query(
        "INSERT INTO oauth_provider_configs (provider, client_id, client_secret, enabled)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (provider) DO UPDATE
            SET client_id = EXCLUDED.client_id,
                client_secret = EXCLUDED.client_secret,
                enabled = EXCLUDED.enabled,
                updated_at = now()",
    )
    .bind(provider.slug())
    .bind(client_id)
    .bind(&sealed)
    .bind(enabled)
    .execute(pool)
    .await?;
    Ok(())
}

// --- Connect states ----------------------------------------------------------

/// How long a pending connect state stays valid.
///
/// A connect is completed as soon as the user consents; ten minutes is ample for a
/// human round-trip while bounding the window a captured state could be replayed in.
const STATE_TTL: Duration = Duration::minutes(10);

/// Persists a pending connect state, keyed by the hash of its opaque `state` value.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn insert_state(
    pool: &PgPool,
    state: &str,
    user_id: Uuid,
    provider: Provider,
    pkce_verifier: Option<&str>,
    redirect_after: &str,
) -> sqlx::Result<()> {
    let expires_at = Utc::now() + STATE_TTL;
    sqlx::query(
        "INSERT INTO oauth_states (state_hash, user_id, provider, pkce_verifier, redirect_after, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(providers::hash_state(state))
    .bind(user_id)
    .bind(provider.slug())
    .bind(pkce_verifier)
    .bind(redirect_after)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// The state recovered from a consumed connect state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingState {
    pub user_id: Uuid,
    pub provider: Provider,
    pub pkce_verifier: Option<String>,
    pub redirect_after: String,
}

/// Atomically consumes a connect state, returning its bound context, or `None`.
///
/// The consume-and-return is a single `DELETE ... RETURNING` guarded on an unexpired
/// row, so a state is single-use even under concurrent callbacks: the second request
/// deletes zero rows and gets `None`. An unknown, expired, or reused state is `None`.
/// A stored provider slug that no longer parses also yields `None` (fail closed).
///
/// # Errors
/// Returns an error if the query fails.
pub async fn consume_state(pool: &PgPool, state: &str) -> sqlx::Result<Option<PendingState>> {
    let row = sqlx::query_as::<_, (Uuid, String, Option<String>, String)>(
        "DELETE FROM oauth_states
          WHERE state_hash = $1 AND expires_at > now()
          RETURNING user_id, provider, pkce_verifier, redirect_after",
    )
    .bind(providers::hash_state(state))
    .fetch_optional(pool)
    .await?;

    Ok(
        row.and_then(|(user_id, provider, pkce_verifier, redirect_after)| {
            Provider::from_slug(&provider).map(|provider| PendingState {
                user_id,
                provider,
                pkce_verifier,
                redirect_after,
            })
        }),
    )
}

// --- Connected accounts ------------------------------------------------------

/// The associated data a connected account's tokens are sealed under.
///
/// Binding to `user:provider:external_id` ties each ciphertext to exactly one
/// account, so a dump can neither open a token nor move it to another account or
/// user (the AEAD authentication fails if the context differs).
fn account_aad(user_id: Uuid, provider: Provider, external_id: &str) -> Vec<u8> {
    format!("{user_id}:{}:{external_id}", provider.slug()).into_bytes()
}

/// Everything needed to link (or re-link) one external account.
pub struct UpsertAccount<'a> {
    pub user_id: Uuid,
    pub provider: Provider,
    pub profile: &'a ExternalProfile,
    pub scopes: &'a [String],
    /// The plaintext access token; sealed before storage.
    pub access_token: &'a str,
    /// The plaintext refresh token when the provider issued one; sealed before storage.
    pub refresh_token: Option<&'a str>,
    pub token_expires_at: Option<DateTime<Utc>>,
}

/// Links or updates an external account, sealing its tokens.
///
/// Re-linking the same external account (same `user_id`/`provider`/`external_id`)
/// updates the row rather than duplicating it, refreshing the display fields, scopes,
/// and tokens and clearing `needs_reconsent`. A re-consent that returns no new
/// refresh token keeps the prior sealed one, so a still-valid refresh token is not
/// dropped. Returns the account id.
///
/// # Errors
/// Returns an error if the upsert fails.
pub async fn upsert_account(
    pool: &PgPool,
    crypto: &Crypto,
    account: UpsertAccount<'_>,
) -> sqlx::Result<Uuid> {
    let aad = account_aad(
        account.user_id,
        account.provider,
        &account.profile.external_id,
    );
    let sealed_access = crypto.seal(account.access_token.as_bytes(), &aad);
    let sealed_refresh = account
        .refresh_token
        .map(|token| crypto.seal(token.as_bytes(), &aad));

    sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO connected_accounts
            (user_id, provider, external_id, external_username, external_email, avatar_url,
             scopes, access_token, refresh_token, token_expires_at, needs_reconsent)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, FALSE)
         ON CONFLICT (user_id, provider, external_id) DO UPDATE
            SET external_username = EXCLUDED.external_username,
                external_email = EXCLUDED.external_email,
                avatar_url = EXCLUDED.avatar_url,
                scopes = EXCLUDED.scopes,
                access_token = EXCLUDED.access_token,
                -- Keep the prior refresh token when a re-consent returns none.
                refresh_token = COALESCE(EXCLUDED.refresh_token, connected_accounts.refresh_token),
                token_expires_at = EXCLUDED.token_expires_at,
                needs_reconsent = FALSE,
                updated_at = now()
         RETURNING id",
    )
    .bind(account.user_id)
    .bind(account.provider.slug())
    .bind(&account.profile.external_id)
    .bind(&account.profile.username)
    .bind(&account.profile.email)
    .bind(&account.profile.avatar_url)
    .bind(account.scopes)
    .bind(&sealed_access)
    .bind(&sealed_refresh)
    .bind(account.token_expires_at)
    .fetch_one(pool)
    .await
}

/// A connected account as shown to its owner: never any token material.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ConnectedAccountView {
    pub id: Uuid,
    pub provider: String,
    pub external_username: String,
    pub external_email: String,
    pub avatar_url: String,
    pub scopes: Vec<String>,
    pub needs_reconsent: bool,
    pub connected_at: DateTime<Utc>,
}

/// Lists a user's connected accounts, newest first (no token material).
///
/// # Errors
/// Returns an error if the query fails.
pub async fn list_accounts(
    pool: &PgPool,
    user_id: Uuid,
) -> sqlx::Result<Vec<ConnectedAccountView>> {
    sqlx::query_as::<_, ConnectedAccountView>(
        "SELECT id, provider, external_username, external_email, avatar_url,
                scopes, needs_reconsent, connected_at
           FROM connected_accounts WHERE user_id = $1 ORDER BY connected_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await
}

/// A removed account's identity, enough to attempt a best-effort upstream revoke.
pub struct RemovedAccount {
    pub provider: Provider,
    pub sealed_access_token: Vec<u8>,
    pub user_id: Uuid,
    pub external_id: String,
}

impl RemovedAccount {
    /// Opens the removed account's access token for a best-effort upstream revoke.
    ///
    /// # Errors
    /// Returns an error if the sealed token cannot be opened.
    pub fn open_access_token(&self, crypto: &Crypto) -> eyre::Result<String> {
        let aad = account_aad(self.user_id, self.provider, &self.external_id);
        let opened = crypto
            .open(&self.sealed_access_token, &aad)
            .map_err(|error| eyre::eyre!("failed to open a connected access token: {error}"))?;
        String::from_utf8(opened.plaintext)
            .map_err(|_| eyre::eyre!("a connected access token is not valid UTF-8"))
    }
}

/// Deletes one of a user's connected accounts, returning its identity when it existed.
///
/// Scoped to `user_id`, so a caller can never delete another user's account: an id
/// that is not theirs matches nothing and yields `None` (the handler maps that to a
/// `404`, never revealing another account's existence).
///
/// # Errors
/// Returns an error if the query fails.
pub async fn delete_account(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> sqlx::Result<Option<RemovedAccount>> {
    let row = sqlx::query_as::<_, (String, Vec<u8>, String)>(
        "DELETE FROM connected_accounts WHERE id = $1 AND user_id = $2
         RETURNING provider, access_token, external_id",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(
        row.and_then(|(provider, sealed_access_token, external_id)| {
            Provider::from_slug(&provider).map(|provider| RemovedAccount {
                provider,
                sealed_access_token,
                user_id,
                external_id,
            })
        }),
    )
}

// --- Grant loading for the access-token resolution seam ----------------------

/// A stored grant with its sealed tokens, for [`super::tokens::resolve_connected_token`].
pub struct StoredGrant {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: Provider,
    pub external_id: String,
    pub sealed_access_token: Vec<u8>,
    pub sealed_refresh_token: Option<Vec<u8>>,
    pub token_expires_at: Option<DateTime<Utc>>,
    pub needs_reconsent: bool,
}

impl StoredGrant {
    /// Opens the sealed access token to its plaintext.
    ///
    /// # Errors
    /// Returns an error if the token cannot be opened.
    pub fn open_access_token(&self, crypto: &Crypto) -> eyre::Result<String> {
        self.open(crypto, &self.sealed_access_token)
    }

    /// Opens the sealed refresh token when present.
    ///
    /// # Errors
    /// Returns an error if a present token cannot be opened.
    pub fn open_refresh_token(&self, crypto: &Crypto) -> eyre::Result<Option<String>> {
        self.sealed_refresh_token
            .as_ref()
            .map(|sealed| self.open(crypto, sealed))
            .transpose()
    }

    fn open(&self, crypto: &Crypto, sealed: &[u8]) -> eyre::Result<String> {
        let aad = account_aad(self.user_id, self.provider, &self.external_id);
        let opened = crypto
            .open(sealed, &aad)
            .map_err(|error| eyre::eyre!("failed to open a connected token: {error}"))?;
        String::from_utf8(opened.plaintext)
            .map_err(|_| eyre::eyre!("a connected token is not valid UTF-8"))
    }
}

/// Loads a connected account's sealed grant by id, or `None` when it is gone.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn load_grant(pool: &PgPool, id: Uuid) -> sqlx::Result<Option<StoredGrant>> {
    let row = sqlx::query_as::<
        _,
        (
            Uuid,
            Uuid,
            String,
            String,
            Vec<u8>,
            Option<Vec<u8>>,
            Option<DateTime<Utc>>,
            bool,
        ),
    >(
        "SELECT id, user_id, provider, external_id, access_token, refresh_token,
                token_expires_at, needs_reconsent
           FROM connected_accounts WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;

    Ok(row.and_then(
        |(id, user_id, provider, external_id, access, refresh, expires, reconsent)| {
            Provider::from_slug(&provider).map(|provider| StoredGrant {
                id,
                user_id,
                provider,
                external_id,
                sealed_access_token: access,
                sealed_refresh_token: refresh,
                token_expires_at: expires,
                needs_reconsent: reconsent,
            })
        },
    ))
}

/// Re-seals a grant's tokens after a successful refresh and clears `needs_reconsent`.
///
/// # Errors
/// Returns an error if the update fails.
pub async fn store_refreshed(
    pool: &PgPool,
    crypto: &Crypto,
    grant: &StoredGrant,
    access_token: &str,
    refresh_token: Option<&str>,
    token_expires_at: Option<DateTime<Utc>>,
) -> sqlx::Result<()> {
    let aad = account_aad(grant.user_id, grant.provider, &grant.external_id);
    let sealed_access = crypto.seal(access_token.as_bytes(), &aad);
    // A refresh that returns no new refresh token keeps the existing one (COALESCE).
    let sealed_refresh = refresh_token.map(|token| crypto.seal(token.as_bytes(), &aad));
    sqlx::query(
        "UPDATE connected_accounts
            SET access_token = $2,
                refresh_token = COALESCE($3, refresh_token),
                token_expires_at = $4,
                needs_reconsent = FALSE,
                updated_at = now()
          WHERE id = $1",
    )
    .bind(grant.id)
    .bind(&sealed_access)
    .bind(&sealed_refresh)
    .bind(token_expires_at)
    .execute(pool)
    .await?;
    Ok(())
}

/// Flags a grant as needing re-consent after a genuinely failed refresh.
///
/// # Errors
/// Returns an error if the update fails.
pub async fn mark_needs_reconsent(pool: &PgPool, id: Uuid) -> sqlx::Result<()> {
    sqlx::query(
        "UPDATE connected_accounts SET needs_reconsent = TRUE, updated_at = now() WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_requires_a_row_and_the_enabled_flag() {
        assert!(!is_configured(None));
        assert!(!is_configured(Some(&ProviderStatus {
            provider: "github".to_string(),
            enabled: false,
        })));
        assert!(is_configured(Some(&ProviderStatus {
            provider: "github".to_string(),
            enabled: true,
        })));
    }

    #[test]
    fn account_aad_is_distinct_per_account_and_stable() {
        let user = Uuid::from_u128(1);
        let other = Uuid::from_u128(2);
        // Stable for the same identity, different across user, provider, or account.
        assert_eq!(
            account_aad(user, Provider::Github, "42"),
            account_aad(user, Provider::Github, "42")
        );
        assert_ne!(
            account_aad(user, Provider::Github, "42"),
            account_aad(other, Provider::Github, "42")
        );
        assert_ne!(
            account_aad(user, Provider::Github, "42"),
            account_aad(user, Provider::Google, "42")
        );
        assert_ne!(
            account_aad(user, Provider::Github, "42"),
            account_aad(user, Provider::Github, "43")
        );
    }

    // --- DB-gated lifecycle --------------------------------------------------
    //
    // These need a reachable postgres; ignored by default, run in CI with
    // `cargo test -- --ignored`. Each seeds a throwaway user so it never collides.

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

    fn test_crypto() -> Crypto {
        Crypto::new(&[5u8; 32]).expect("32-byte key is valid")
    }

    fn sample_profile(external_id: &str, email: &str) -> ExternalProfile {
        ExternalProfile {
            external_id: external_id.to_string(),
            username: "octocat".to_string(),
            email: email.to_string(),
            avatar_url: "https://avatars/1".to_string(),
        }
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn state_is_single_use_and_expires() {
        let pool = connect_test_pool().await;
        let user_id = seed_user(&pool).await;
        let state = providers::generate_state();

        insert_state(
            &pool,
            &state,
            user_id,
            Provider::Google,
            Some("verifier-1"),
            "/settings",
        )
        .await
        .expect("insert state");

        // First consume returns the bound context.
        let pending = consume_state(&pool, &state)
            .await
            .expect("consume query")
            .expect("a fresh state consumes");
        assert_eq!(pending.user_id, user_id);
        assert_eq!(pending.provider, Provider::Google);
        assert_eq!(pending.pkce_verifier.as_deref(), Some("verifier-1"));
        assert_eq!(pending.redirect_after, "/settings");
        // A second consume of the same state is refused (single-use).
        assert!(consume_state(&pool, &state)
            .await
            .expect("consume query")
            .is_none());

        // An expired state never consumes.
        let expired = providers::generate_state();
        insert_state(
            &pool,
            &expired,
            user_id,
            Provider::Github,
            None,
            "/settings",
        )
        .await
        .expect("insert state");
        sqlx::query("UPDATE oauth_states SET expires_at = now() - interval '1 minute' WHERE state_hash = $1")
            .bind(providers::hash_state(&expired))
            .execute(&pool)
            .await
            .expect("expire state");
        assert!(consume_state(&pool, &expired)
            .await
            .expect("consume query")
            .is_none());
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn relinking_the_same_account_updates_rather_than_duplicates() {
        let pool = connect_test_pool().await;
        let crypto = test_crypto();
        let user_id = seed_user(&pool).await;

        let first = upsert_account(
            &pool,
            &crypto,
            UpsertAccount {
                user_id,
                provider: Provider::Github,
                profile: &sample_profile("42", "old@example.com"),
                scopes: &["read:user".to_string()],
                access_token: "access-1",
                refresh_token: Some("refresh-1"),
                token_expires_at: None,
            },
        )
        .await
        .expect("first link");

        // Re-linking the same external id updates the same row (no new refresh token).
        let second = upsert_account(
            &pool,
            &crypto,
            UpsertAccount {
                user_id,
                provider: Provider::Github,
                profile: &sample_profile("42", "new@example.com"),
                scopes: &["read:user".to_string(), "user:email".to_string()],
                access_token: "access-2",
                refresh_token: None,
                token_expires_at: None,
            },
        )
        .await
        .expect("re-link");
        assert_eq!(first, second, "re-link must update the same row");

        let accounts = list_accounts(&pool, user_id).await.expect("list");
        assert_eq!(accounts.len(), 1, "re-linking must not duplicate");
        assert_eq!(accounts[0].external_email, "new@example.com");
        assert_eq!(accounts[0].scopes, vec!["read:user", "user:email"]);

        // The prior refresh token survives a re-consent that returned none, and the
        // access token opens to the new value (proving the reseal + AAD binding).
        let grant = load_grant(&pool, second)
            .await
            .expect("load")
            .expect("grant exists");
        assert_eq!(grant.open_access_token(&crypto).expect("open"), "access-2");
        assert_eq!(
            grant.open_refresh_token(&crypto).expect("open"),
            Some("refresh-1".to_string())
        );
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn a_user_cannot_delete_another_users_account() {
        let pool = connect_test_pool().await;
        let crypto = test_crypto();
        let owner = seed_user(&pool).await;
        let intruder = seed_user(&pool).await;

        let account_id = upsert_account(
            &pool,
            &crypto,
            UpsertAccount {
                user_id: owner,
                provider: Provider::Github,
                profile: &sample_profile("99", "owner@example.com"),
                scopes: &[],
                access_token: "access",
                refresh_token: None,
                token_expires_at: None,
            },
        )
        .await
        .expect("link");

        // The intruder deleting the owner's account matches nothing.
        assert!(delete_account(&pool, intruder, account_id)
            .await
            .expect("delete query")
            .is_none());
        // The account still exists for its owner.
        assert_eq!(list_accounts(&pool, owner).await.expect("list").len(), 1);
        // The owner can delete it, and the returned identity opens its token.
        let removed = delete_account(&pool, owner, account_id)
            .await
            .expect("delete query")
            .expect("owner deletes their own");
        assert_eq!(removed.open_access_token(&crypto).expect("open"), "access");
        assert!(list_accounts(&pool, owner).await.expect("list").is_empty());
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn setting_provider_credentials_flips_configured() {
        let pool = connect_test_pool().await;
        let crypto = test_crypto();

        // Unconfigured to start.
        let before = provider_statuses(&pool).await.expect("statuses");
        let github_before = before.iter().find(|s| s.provider == "github");
        assert!(!is_configured(github_before));

        // An admin sets and enables the credentials.
        upsert_provider_config(
            &pool,
            &crypto,
            Provider::Github,
            "gh-client",
            "gh-secret",
            true,
        )
        .await
        .expect("set config");

        let after = provider_statuses(&pool).await.expect("statuses");
        let github_after = after.iter().find(|s| s.provider == "github");
        assert!(
            is_configured(github_after),
            "enabled credentials are configured"
        );

        // The secret round-trips through the seal, and stays out of the status list.
        let config = provider_config(&pool, &crypto, Provider::Github)
            .await
            .expect("config query")
            .expect("configured");
        assert_eq!(config.client_id, "gh-client");
        assert_eq!(config.client_secret, "gh-secret");
        assert!(config.enabled);

        // Disabling it un-configures the provider without deleting the row.
        upsert_provider_config(
            &pool,
            &crypto,
            Provider::Github,
            "gh-client",
            "gh-secret",
            false,
        )
        .await
        .expect("disable");
        let disabled = provider_statuses(&pool).await.expect("statuses");
        assert!(!is_configured(
            disabled.iter().find(|s| s.provider == "github")
        ));
    }
}
