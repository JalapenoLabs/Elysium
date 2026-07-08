//! Application users: the auth system of record.
//!
//! Auth is API-owned (see `docs/auth-model.md`), so users live here, not in
//! Keycloak. This module is the small set of lookups and provisioning the auth
//! flow needs.

use sqlx::PgPool;
use uuid::Uuid;

/// A user record used by the auth flow.
#[derive(Debug, Clone)]
pub struct User {
    pub id: Uuid,
    pub email: String,
}

/// Row shape shared by the user lookups.
type UserRow = (Uuid, String);

fn into_user(row: UserRow) -> User {
    User {
        id: row.0,
        email: row.1,
    }
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
    let row =
        sqlx::query_as::<_, UserRow>("SELECT id, email FROM users WHERE lower(email) = lower($1)")
            .bind(email)
            .fetch_optional(pool)
            .await?;

    Ok(row.map(into_user))
}

/// Creates the first (root) user together with a root organization.
///
/// # Errors
/// Returns an error if the inserts fail (e.g. the email is already taken).
pub async fn create_root(pool: &PgPool, email: &str, display_name: &str) -> sqlx::Result<User> {
    let mut tx = pool.begin().await?;

    let org_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO orgs (name, slug) VALUES ('Root', 'root') RETURNING id",
    )
    .fetch_one(&mut *tx)
    .await?;

    // The subject is an opaque, stable identifier we own (not from any external IdP).
    let subject = Uuid::new_v4().to_string();
    let row = sqlx::query_as::<_, UserRow>(
        "INSERT INTO users (subject, org_id, email, display_name)
         VALUES ($1, $2, $3, $4)
         RETURNING id, email",
    )
    .bind(&subject)
    .bind(org_id)
    .bind(email)
    .bind(display_name)
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(into_user(row))
}
