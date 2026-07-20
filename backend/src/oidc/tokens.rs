//! Token issuance: claims, PKCE, authorization codes, refresh tokens, JWT minting.
//!
//! This module turns an authenticated user into the tokens an OIDC client receives:
//! a signed ID token (who the user is), a signed access token (what the bearer may
//! call), and, when `offline_access` is granted, an opaque refresh token to renew
//! them. The identity claims (subject, email, name, org, roles/groups) are loaded
//! from Elysium's own tables, the source of truth, so a downstream service such as
//! Forgejo maps them straight onto its accounts and teams.
//!
//! Two bearer artifacts are hashed at rest, never stored in the clear: the
//! authorization code and the refresh token. Both are single-use, the code by a
//! consume-on-redeem update and the refresh token by rotation, so a captured value
//! cannot be replayed. PKCE binds a code to the client instance that requested it,
//! closing the authorization-code interception attack for public clients.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::Utc;
use rand::rngs::OsRng;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

use crate::oidc::keys::{self, hash_secret};
use crate::state::AppState;

/// Authorization-code lifetime: a code is redeemed immediately after the redirect,
/// so a tight window bounds the replay surface (OIDC recommends <= 10 minutes; 60s
/// is ample for a same-machine redirect and much tighter).
const AUTH_CODE_TTL_SECONDS: i64 = 60;

/// ID-token lifetime: short, because it only proves the sign-in that just happened.
const ID_TOKEN_TTL_SECONDS: i64 = 300;

/// Access-token lifetime: the working session for API calls, renewed via refresh.
const ACCESS_TOKEN_TTL_SECONDS: i64 = 3600;

/// Refresh-token lifetime: long-lived offline access, still single-use per rotation.
const REFRESH_TOKEN_TTL_SECONDS: i64 = 30 * 24 * 60 * 60;

/// CSPRNG entropy in a generated code or refresh token, in bytes (256 bits).
const SECRET_ENTROPY_BYTES: usize = 32;

// --- PKCE --------------------------------------------------------------------

/// Verifies a PKCE `code_verifier` against the stored `challenge` and `method`.
///
/// Only S256 is supported: the challenge must equal the base64url (unpadded)
/// SHA-256 of the verifier (RFC 7636). A `plain` method, a mismatched verifier, or
/// any unknown method fails closed. The comparison is constant-time so a mismatch
/// leaks no timing signal.
pub fn verify_pkce(verifier: &str, challenge: &str, method: &str) -> bool {
    if method != "S256" {
        return false;
    }
    let computed = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    crate::oidc::constant_time_eq(computed.as_bytes(), challenge.as_bytes())
}

// --- Identity claims ---------------------------------------------------------

/// The active org context stamped into a token: which org, and the caller's role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrgClaim {
    pub id: String,
    pub slug: String,
    pub role: String,
}

/// Everything about a user a token can carry, loaded from Elysium's own tables.
///
/// `roles` are the org-level role tokens (e.g. `["admin"]`) and `groups` are the
/// user's team slugs in the active org, the two axes a downstream service maps onto
/// its own roles and teams.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub user_id: Uuid,
    pub email: String,
    pub email_verified: bool,
    pub name: String,
    pub given_name: String,
    pub family_name: String,
    pub org: Option<OrgClaim>,
    pub groups: Vec<String>,
    pub roles: Vec<String>,
}

/// The JWT claim set for an ID or access token.
///
/// Optional identity claims are omitted (not null) when their scope was not granted
/// or the value is empty, so a token carries exactly what the client asked for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub iss: String,
    pub sub: String,
    pub aud: String,
    pub iat: i64,
    pub exp: i64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub nonce: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub email_verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub given_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub family_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub org: Option<OrgClaim>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub groups: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub roles: Vec<String>,
}

/// Whether a scope was granted (a plain membership test over the granted set).
fn granted(scopes: &[String], scope: &str) -> bool {
    scopes.iter().any(|granted| granted == scope)
}

/// Builds a claim set for `subject`, gated by the granted `scopes`.
///
/// `iss`/`aud`/`iat`/`exp` are always present. The `profile` scope adds
/// name/given_name/family_name and the org/roles/groups claims; the `email` scope
/// adds email/email_verified. `nonce` is echoed only for the ID token (the caller
/// passes `Some` there, `None` for the access token). `now` and `ttl` are explicit
/// so the construction is deterministic and unit-testable without a clock.
pub fn build_claims(
    subject: &Subject,
    issuer: &str,
    audience: &str,
    scopes: &[String],
    nonce: Option<&str>,
    now: i64,
    ttl: i64,
) -> Claims {
    let profile = granted(scopes, "profile");
    let non_empty = |value: &str| (!value.is_empty()).then(|| value.to_string());

    Claims {
        iss: issuer.to_string(),
        sub: subject.user_id.to_string(),
        aud: audience.to_string(),
        iat: now,
        exp: now + ttl,
        nonce: nonce.map(str::to_string),
        email: granted(scopes, "email").then(|| subject.email.clone()),
        email_verified: granted(scopes, "email").then_some(subject.email_verified),
        name: profile.then(|| non_empty(&subject.name)).flatten(),
        given_name: profile.then(|| non_empty(&subject.given_name)).flatten(),
        family_name: profile.then(|| non_empty(&subject.family_name)).flatten(),
        org: profile.then(|| subject.org.clone()).flatten(),
        groups: if profile {
            subject.groups.clone()
        } else {
            Vec::new()
        },
        roles: if profile {
            subject.roles.clone()
        } else {
            Vec::new()
        },
    }
}

/// Loads a user's identity claims for `org_id`, or `None` when the user is gone.
///
/// The org/role/group claims are scoped to `org_id` (the token's org context); a
/// `None` org yields no org, role, or group claims. `email_verified` is `true`
/// because email is Elysium's global profile key and the source of auth truth: an
/// account cannot exist without a claimed, unique email.
///
/// # Errors
/// Returns an error if the database is unreachable.
pub async fn load_subject(
    pool: &PgPool,
    user_id: Uuid,
    org_id: Option<Uuid>,
) -> sqlx::Result<Option<Subject>> {
    let user = sqlx::query_as::<_, (String, String, String, String)>(
        "SELECT email, display_name, given_name, family_name FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;
    let Some((email, display_name, given_name, family_name)) = user else {
        return Ok(None);
    };

    // The org context and the caller's role in it; None org -> no org claims.
    let (org, roles) = match org_id {
        Some(org_id) => {
            let row = sqlx::query_as::<_, (String, String)>(
                "SELECT o.slug, m.role FROM org_members m JOIN orgs o ON o.id = m.org_id
                  WHERE m.user_id = $1 AND m.org_id = $2",
            )
            .bind(user_id)
            .bind(org_id)
            .fetch_optional(pool)
            .await?;
            match row {
                Some((slug, role)) => (
                    Some(OrgClaim {
                        id: org_id.to_string(),
                        slug,
                        role: role.clone(),
                    }),
                    vec![role],
                ),
                None => (None, Vec::new()),
            }
        }
        None => (None, Vec::new()),
    };

    // Team slugs within the active org, the group axis a service maps to its teams.
    let groups = match org_id {
        Some(org_id) => {
            sqlx::query_scalar::<_, String>(
                "SELECT t.slug FROM team_members tm JOIN teams t ON t.id = tm.team_id
              WHERE tm.user_id = $1 AND t.org_id = $2 ORDER BY t.slug",
            )
            .bind(user_id)
            .bind(org_id)
            .fetch_all(pool)
            .await?
        }
        None => Vec::new(),
    };

    // display_name is the rendered name; fall back to the given name when blank.
    let name = if display_name.is_empty() {
        given_name.clone()
    } else {
        display_name
    };

    Ok(Some(Subject {
        user_id,
        email,
        email_verified: true,
        name,
        given_name,
        family_name,
        org,
        groups,
        roles,
    }))
}

// --- Authorization codes -----------------------------------------------------

/// The details bound to an authorization code when it is minted.
pub struct NewCode<'a> {
    pub client_id: &'a str,
    pub user_id: Uuid,
    pub org_id: Option<Uuid>,
    pub redirect_uri: &'a str,
    pub scopes: &'a [String],
    pub code_challenge: &'a str,
    pub code_challenge_method: &'a str,
    pub nonce: Option<&'a str>,
}

/// Mints and stores a single-use authorization code, returning the plaintext.
///
/// Only the code's hash is persisted; the plaintext is returned once to ride the
/// redirect back to the client. It expires in [`AUTH_CODE_TTL_SECONDS`].
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn issue_code(pool: &PgPool, code: NewCode<'_>) -> sqlx::Result<String> {
    let secret = generate_secret();
    let expires_at = Utc::now() + chrono::Duration::seconds(AUTH_CODE_TTL_SECONDS);
    sqlx::query(
        "INSERT INTO oidc_authorization_codes
            (code_hash, client_id, user_id, org_id, redirect_uri, scopes,
             code_challenge, code_challenge_method, nonce, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(hash_secret(&secret))
    .bind(code.client_id)
    .bind(code.user_id)
    .bind(code.org_id)
    .bind(code.redirect_uri)
    .bind(code.scopes)
    .bind(code.code_challenge)
    .bind(code.code_challenge_method)
    .bind(code.nonce)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(secret)
}

/// The state recovered from a redeemed authorization code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemedCode {
    pub client_id: String,
    pub user_id: Uuid,
    pub org_id: Option<Uuid>,
    pub redirect_uri: String,
    pub scopes: Vec<String>,
    pub code_challenge: String,
    pub code_challenge_method: String,
    pub nonce: Option<String>,
}

/// Atomically consumes a code, returning its bound state, or `None` when invalid.
///
/// The consume-and-return is a single `UPDATE ... RETURNING` guarded on
/// `consumed_at IS NULL` and unexpired, so a code redeems exactly once even under
/// concurrent requests: the second request updates zero rows and gets `None`.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn redeem_code(pool: &PgPool, code: &str) -> sqlx::Result<Option<RedeemedCode>> {
    let row = sqlx::query_as::<
        _,
        (
            String,
            Uuid,
            Option<Uuid>,
            String,
            Vec<String>,
            String,
            String,
            Option<String>,
        ),
    >(
        "UPDATE oidc_authorization_codes SET consumed_at = now()
          WHERE code_hash = $1 AND consumed_at IS NULL AND expires_at > now()
          RETURNING client_id, user_id, org_id, redirect_uri, scopes,
                    code_challenge, code_challenge_method, nonce",
    )
    .bind(hash_secret(code))
    .fetch_optional(pool)
    .await?;

    Ok(row.map(
        |(client_id, user_id, org_id, redirect_uri, scopes, code_challenge, method, nonce)| {
            RedeemedCode {
                client_id,
                user_id,
                org_id,
                redirect_uri,
                scopes,
                code_challenge,
                code_challenge_method: method,
                nonce,
            }
        },
    ))
}

// --- Refresh tokens ----------------------------------------------------------

/// Mints and stores a single-use refresh token, returning the plaintext.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn issue_refresh_token(
    pool: &PgPool,
    client_id: &str,
    user_id: Uuid,
    org_id: Option<Uuid>,
    scopes: &[String],
    nonce: Option<&str>,
) -> sqlx::Result<String> {
    let secret = generate_secret();
    let expires_at = Utc::now() + chrono::Duration::seconds(REFRESH_TOKEN_TTL_SECONDS);
    sqlx::query(
        "INSERT INTO oidc_refresh_tokens
            (token_hash, client_id, user_id, org_id, scopes, nonce, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(hash_secret(&secret))
    .bind(client_id)
    .bind(user_id)
    .bind(org_id)
    .bind(scopes)
    .bind(nonce)
    .bind(expires_at)
    .execute(pool)
    .await?;
    Ok(secret)
}

/// The state recovered from a redeemed refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemedRefresh {
    pub user_id: Uuid,
    pub org_id: Option<Uuid>,
    pub scopes: Vec<String>,
    pub nonce: Option<String>,
}

/// Atomically consumes a refresh token for `client_id`, or `None` when invalid.
///
/// Consumed on use (rotation): the caller issues a fresh token, so a captured
/// refresh token is single-use. The `client_id` guard binds a token to the client
/// it was issued to, so one client cannot redeem another's token.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn redeem_refresh_token(
    pool: &PgPool,
    client_id: &str,
    token: &str,
) -> sqlx::Result<Option<RedeemedRefresh>> {
    let row = sqlx::query_as::<_, (Uuid, Option<Uuid>, Vec<String>, Option<String>)>(
        "UPDATE oidc_refresh_tokens SET consumed_at = now()
          WHERE token_hash = $1 AND client_id = $2 AND consumed_at IS NULL
            AND (expires_at IS NULL OR expires_at > now())
          RETURNING user_id, org_id, scopes, nonce",
    )
    .bind(hash_secret(token))
    .bind(client_id)
    .fetch_optional(pool)
    .await?;

    Ok(row.map(|(user_id, org_id, scopes, nonce)| RedeemedRefresh {
        user_id,
        org_id,
        scopes,
        nonce,
    }))
}

// --- Token issuance ----------------------------------------------------------

/// The full set of tokens returned from the token endpoint.
pub struct IssuedTokens {
    pub access_token: String,
    pub id_token: String,
    /// Present only when `offline_access` was granted.
    pub refresh_token: Option<String>,
    pub expires_in: i64,
    pub scope: String,
}

/// Signs the ID and access tokens (and, for `offline_access`, a refresh token).
///
/// Both JWTs are signed with the active key via [`keys::sign`]; the ID token echoes
/// `nonce` and lives [`ID_TOKEN_TTL_SECONDS`], the access token lives
/// [`ACCESS_TOKEN_TTL_SECONDS`]. The `aud` of both is the client id.
///
/// # Errors
/// Returns an error if signing or the refresh-token insert fails.
pub async fn issue_tokens(
    state: &AppState,
    client_id: &str,
    subject: &Subject,
    scopes: &[String],
    nonce: Option<&str>,
) -> eyre::Result<IssuedTokens> {
    let now = Utc::now().timestamp();
    let issuer = &state.app_origin;

    let id_claims = build_claims(
        subject,
        issuer,
        client_id,
        scopes,
        nonce,
        now,
        ID_TOKEN_TTL_SECONDS,
    );
    let access_claims = build_claims(
        subject,
        issuer,
        client_id,
        scopes,
        None,
        now,
        ACCESS_TOKEN_TTL_SECONDS,
    );

    let id_token = keys::sign(&state.pool, &state.crypto, &id_claims).await?;
    let access_token = keys::sign(&state.pool, &state.crypto, &access_claims).await?;

    let refresh_token = if granted(scopes, "offline_access") {
        Some(
            issue_refresh_token(
                &state.pool,
                client_id,
                subject.user_id,
                org_id_of(subject),
                scopes,
                nonce,
            )
            .await?,
        )
    } else {
        None
    };

    Ok(IssuedTokens {
        access_token,
        id_token,
        refresh_token,
        expires_in: ACCESS_TOKEN_TTL_SECONDS,
        scope: scopes.join(" "),
    })
}

/// The org id carried by a subject's org claim, if any (parsed back to a `Uuid`).
fn org_id_of(subject: &Subject) -> Option<Uuid> {
    subject
        .org
        .as_ref()
        .and_then(|org| Uuid::parse_str(&org.id).ok())
}

/// Generates a fresh opaque secret: 256 CSPRNG bits, base64url without padding.
fn generate_secret() -> String {
    let mut entropy = [0u8; SECRET_ENTROPY_BYTES];
    OsRng.fill_bytes(&mut entropy);
    URL_SAFE_NO_PAD.encode(entropy)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fully-populated subject for claim-construction tests.
    fn sample_subject() -> Subject {
        Subject {
            user_id: Uuid::from_u128(0x1234),
            email: "alex@example.test".to_string(),
            email_verified: true,
            name: "Alex Rivera".to_string(),
            given_name: "Alex".to_string(),
            family_name: "Rivera".to_string(),
            org: Some(OrgClaim {
                id: Uuid::from_u128(0xABCD).to_string(),
                slug: "navarrocity".to_string(),
                role: "admin".to_string(),
            }),
            groups: vec!["dev".to_string(), "qa".to_string()],
            roles: vec!["admin".to_string()],
        }
    }

    fn all_scopes() -> Vec<String> {
        ["openid", "profile", "email", "offline_access"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    // --- PKCE ----------------------------------------------------------------

    #[test]
    fn pkce_s256_accepts_a_matching_verifier() {
        // A known RFC 7636 vector: verifier and its base64url(SHA256) challenge.
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        assert!(verify_pkce(verifier, &challenge, "S256"));
    }

    #[test]
    fn pkce_rejects_a_mismatched_verifier() {
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(b"the-real-verifier"));
        assert!(!verify_pkce("a-different-verifier", &challenge, "S256"));
    }

    #[test]
    fn pkce_rejects_the_plain_method() {
        // Even when verifier == challenge, only S256 is accepted; plain is refused.
        assert!(!verify_pkce("same-value", "same-value", "plain"));
        assert!(!verify_pkce("x", "x", ""));
    }

    // --- Claim construction --------------------------------------------------

    #[test]
    fn claims_carry_issuer_subject_audience_and_expiry() {
        let subject = sample_subject();
        let claims = build_claims(
            &subject,
            "https://elysium.example",
            "forgejo-client",
            &all_scopes(),
            Some("nonce-123"),
            1_000,
            300,
        );
        assert_eq!(claims.iss, "https://elysium.example");
        // sub is the stable user id (uuid string), not the email.
        assert_eq!(claims.sub, subject.user_id.to_string());
        assert_eq!(claims.aud, "forgejo-client");
        assert_eq!(claims.iat, 1_000);
        assert_eq!(claims.exp, 1_300);
        // The nonce is echoed for the ID token.
        assert_eq!(claims.nonce.as_deref(), Some("nonce-123"));
    }

    #[test]
    fn claims_include_email_and_profile_when_scoped() {
        let subject = sample_subject();
        let claims = build_claims(&subject, "iss", "aud", &all_scopes(), None, 0, 300);
        assert_eq!(claims.email.as_deref(), Some("alex@example.test"));
        assert_eq!(claims.email_verified, Some(true));
        assert_eq!(claims.name.as_deref(), Some("Alex Rivera"));
        assert_eq!(claims.given_name.as_deref(), Some("Alex"));
        assert_eq!(claims.family_name.as_deref(), Some("Rivera"));
        // Org and role/group claims ride the profile scope.
        assert_eq!(
            claims.org.as_ref().map(|o| o.slug.as_str()),
            Some("navarrocity")
        );
        assert_eq!(claims.org.as_ref().map(|o| o.role.as_str()), Some("admin"));
        assert_eq!(claims.roles, vec!["admin".to_string()]);
        assert_eq!(claims.groups, vec!["dev".to_string(), "qa".to_string()]);
        // The access token carries no nonce.
        assert_eq!(claims.nonce, None);
    }

    #[test]
    fn claims_omit_email_and_profile_when_not_scoped() {
        let subject = sample_subject();
        // Only openid granted: no email, no profile-derived claims.
        let claims = build_claims(
            &subject,
            "iss",
            "aud",
            &["openid".to_string()],
            None,
            0,
            300,
        );
        assert_eq!(claims.email, None);
        assert_eq!(claims.email_verified, None);
        assert_eq!(claims.name, None);
        assert_eq!(claims.org, None);
        assert!(claims.roles.is_empty());
        assert!(claims.groups.is_empty());
    }

    #[test]
    fn claims_serialize_without_absent_optional_fields() {
        let subject = sample_subject();
        let claims = build_claims(
            &subject,
            "iss",
            "aud",
            &["openid".to_string()],
            None,
            0,
            300,
        );
        let json = serde_json::to_value(&claims).expect("claims serialize");
        // Absent claims are omitted entirely, not serialized as null.
        assert!(json.get("email").is_none());
        assert!(json.get("nonce").is_none());
        assert!(json.get("groups").is_none());
        // Mandatory claims are always present.
        assert!(json.get("iss").is_some());
        assert!(json.get("sub").is_some());
        assert!(json.get("exp").is_some());
    }

    // --- DB-gated code and refresh-token lifecycle ---------------------------
    //
    // Need a reachable postgres; ignored by default, run in CI with
    // `cargo test -- --ignored`. Each test seeds its own throwaway user and
    // client, so they never collide with other DB tests.

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

    /// Seeds a throwaway user and a first-party client, returning their ids.
    async fn seed_user_and_client(pool: &PgPool) -> (Uuid, String) {
        let suffix = Uuid::new_v4();
        let user_id = sqlx::query_scalar::<_, Uuid>(
            "INSERT INTO users (subject, email) VALUES ($1, $2) RETURNING id",
        )
        .bind(format!("subject-{suffix}"))
        .bind(format!("user-{suffix}@example.test"))
        .fetch_one(pool)
        .await
        .expect("seed user");

        let client_id = format!("client-{suffix}");
        sqlx::query(
            "INSERT INTO oidc_clients (client_id, name, redirect_uris, scopes, grant_types, is_first_party)
             VALUES ($1, 'Test', ARRAY['https://svc.example/cb'],
                     ARRAY['openid','profile','offline_access'],
                     ARRAY['authorization_code','refresh_token'], TRUE)",
        )
        .bind(&client_id)
        .execute(pool)
        .await
        .expect("seed client");

        (user_id, client_id)
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn authorization_code_is_single_use_and_expires() {
        let pool = connect_test_pool().await;
        let (user_id, client_id) = seed_user_and_client(&pool).await;
        let scopes = vec!["openid".to_string()];

        let code = issue_code(
            &pool,
            NewCode {
                client_id: &client_id,
                user_id,
                org_id: None,
                redirect_uri: "https://svc.example/cb",
                scopes: &scopes,
                code_challenge: "challenge",
                code_challenge_method: "S256",
                nonce: Some("n-1"),
            },
        )
        .await
        .expect("issue code");

        // The first redemption returns the bound state.
        let redeemed = redeem_code(&pool, &code)
            .await
            .expect("redeem query")
            .expect("a fresh code redeems");
        assert_eq!(redeemed.client_id, client_id);
        assert_eq!(redeemed.user_id, user_id);
        assert_eq!(redeemed.nonce.as_deref(), Some("n-1"));
        // A second redemption of the same code is refused (single-use).
        assert!(redeem_code(&pool, &code)
            .await
            .expect("redeem query")
            .is_none());

        // A code past its expiry never redeems, even unconsumed.
        let expired = issue_code(
            &pool,
            NewCode {
                client_id: &client_id,
                user_id,
                org_id: None,
                redirect_uri: "https://svc.example/cb",
                scopes: &scopes,
                code_challenge: "challenge",
                code_challenge_method: "S256",
                nonce: None,
            },
        )
        .await
        .expect("issue code");
        sqlx::query(
            "UPDATE oidc_authorization_codes SET expires_at = now() - interval '1 minute'
              WHERE code_hash = $1",
        )
        .bind(hash_secret(&expired))
        .execute(&pool)
        .await
        .expect("expire code");
        assert!(redeem_code(&pool, &expired)
            .await
            .expect("redeem query")
            .is_none());
    }

    #[tokio::test]
    #[ignore = "requires a postgres reachable via DATABASE_URL"]
    async fn refresh_token_is_single_use_and_client_bound() {
        let pool = connect_test_pool().await;
        let (user_id, client_id) = seed_user_and_client(&pool).await;
        let scopes = vec!["openid".to_string(), "offline_access".to_string()];

        let refresh = issue_refresh_token(&pool, &client_id, user_id, None, &scopes, None)
            .await
            .expect("issue refresh");

        // Another client cannot redeem a token issued to this one.
        assert!(redeem_refresh_token(&pool, "someone-else", &refresh)
            .await
            .expect("redeem query")
            .is_none());

        // The owning client redeems it once (rotation consumes it).
        let redeemed = redeem_refresh_token(&pool, &client_id, &refresh)
            .await
            .expect("redeem query")
            .expect("a fresh refresh token redeems");
        assert_eq!(redeemed.user_id, user_id);
        assert_eq!(redeemed.scopes, scopes);
        // A second redemption is refused: the token is single-use.
        assert!(redeem_refresh_token(&pool, &client_id, &refresh)
            .await
            .expect("redeem query")
            .is_none());
    }
}
