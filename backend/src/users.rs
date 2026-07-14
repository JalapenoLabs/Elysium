//! Application users: the auth system of record.
//!
//! Auth is API-owned (see `docs/auth-model.md`), so users live here, in the
//! application database. This module is the small set of lookups and
//! provisioning the auth flow needs.

use eyre::WrapErr;
use sqlx::{PgConnection, PgPool};
use uuid::Uuid;

use crate::crypto::Crypto;

/// A user record used by the auth flow.
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
}

/// Whether any user exists yet, which distinguishes first-run claim from sign-in.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn any_exist(pool: &PgPool) -> sqlx::Result<bool> {
    let count = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM users")
        .fetch_one(pool)
        .await?;
    Ok(count > 0)
}

/// Finds a user by email, case-insensitively.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn find_by_email(pool: &PgPool, email: &str) -> sqlx::Result<Option<User>> {
    let id = sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE lower(email) = lower($1)")
        .bind(email)
        .fetch_optional(pool)
        .await?;

    Ok(id.map(|id| User { id }))
}

/// Resolves a session subject to the profile's id.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn id_for_subject(pool: &PgPool, subject: &str) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM users WHERE subject = $1")
        .bind(subject)
        .fetch_optional(pool)
        .await
}

/// The org a profile defaults to (its earliest membership), if any.
///
/// A profile can belong to many orgs (see `org_members`); this picks the one a
/// fresh session lands on until the user switches.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn default_org(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Option<Uuid>> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT org_id FROM org_members WHERE user_id = $1 ORDER BY joined_at ASC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Adds a profile to an org with a role, on the caller's connection.
///
/// Idempotent: an existing membership keeps its role. Runs inside the caller's
/// transaction so joining is atomic with whatever provisioned the profile.
///
/// # Errors
/// Returns an error if the insert fails.
pub async fn add_membership(
    conn: &mut PgConnection,
    user_id: Uuid,
    org_id: Uuid,
    role: &str,
) -> sqlx::Result<()> {
    sqlx::query(
        "INSERT INTO org_members (user_id, org_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (user_id, org_id) DO NOTHING",
    )
    .bind(user_id)
    .bind(org_id)
    .bind(role)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Loads the stored OPAQUE password record for a user, if enrolled.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn password_record(pool: &PgPool, user_id: Uuid) -> sqlx::Result<Option<Vec<u8>>> {
    sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT record FROM user_password_credentials WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
}

/// Whether a user has a confirmed TOTP second factor enrolled.
///
/// A presence check that never decrypts, for deciding whether a password sign-in
/// still needs the second-factor step.
///
/// # Errors
/// Returns an error if the query fails.
pub async fn has_totp(pool: &PgPool, user_id: Uuid) -> sqlx::Result<bool> {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (
             SELECT 1 FROM user_totp_secrets WHERE user_id = $1 AND confirmed_at IS NOT NULL
         )",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await
}

/// Loads and decrypts the confirmed TOTP secret for a user, if one is enrolled.
///
/// The secret is sealed at rest (see [`crate::crypto`]); a row written before
/// encryption existed is read as legacy plaintext and resealed in place, so the
/// column converges to all-encrypted without a migration step.
///
/// # Errors
/// Returns an error if the query fails or the stored secret cannot be decrypted
/// (wrong key or tampering).
pub async fn totp_secret(
    pool: &PgPool,
    crypto: &Crypto,
    user_id: Uuid,
) -> eyre::Result<Option<Vec<u8>>> {
    let Some(stored) = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT secret FROM user_totp_secrets WHERE user_id = $1 AND confirmed_at IS NOT NULL",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await
    .wrap_err("failed to read TOTP secret")?
    else {
        return Ok(None);
    };

    let opened = crypto
        .open(&stored, user_id.as_bytes())
        .wrap_err("failed to open the stored TOTP secret")?;
    if !opened.was_sealed {
        let sealed = crypto.seal(&opened.plaintext, user_id.as_bytes());
        sqlx::query("UPDATE user_totp_secrets SET secret = $1 WHERE user_id = $2")
            .bind(&sealed)
            .bind(user_id)
            .execute(pool)
            .await
            .wrap_err("failed to reseal the TOTP secret")?;
    }
    Ok(Some(opened.plaintext))
}
