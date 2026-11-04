// Copyright © 2026 Jalapeno Labs

//! The new authenticator app a person is moving to, held until a code from it is verified.
//!
//! Kratos holds one app per identity and offers a new one only once the old one is removed, so
//! moving apps through Kratos alone would leave two-factor authentication off in between.
//! Instead Elysium holds the new app's secret here, sealed, while the person scans it, and
//! `crate::auth::authenticator_change` swaps it into Kratos once a code from it checks out. One
//! row per person: starting again replaces it. A row lives for
//! [`crate::auth::authenticator_change::REPLACEMENT_LIFETIME`]; an expired one is never read, and
//! every start removes all of them. See `docs/auth.md`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::SecretSlice;
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::authenticator_replacements;

/// Associated data binding a sealed secret to its person, so a ciphertext copied into another
/// person's row refuses to open.
fn secret_context(user_id: Uuid) -> Vec<u8> {
    let mut context = b"authenticator_replacements.secret:".to_vec();
    context.extend_from_slice(user_id.as_bytes());
    context
}

/// Holds `secret` as the person's new app until `expires_at`, replacing one they started before.
/// Every row already expired goes in the same step, so a secret nobody came back for is not kept.
///
/// # Errors
/// Propagates any database error.
pub async fn start(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    user_id: Uuid,
    secret: &[u8],
    expires_at: DateTime<Utc>,
) -> QueryResult<()> {
    diesel::delete(
        authenticator_replacements::table
            .filter(authenticator_replacements::expires_at.le(Utc::now())),
    )
    .execute(connection)
    .await?;

    let sealed = cipher.seal(secret, &secret_context(user_id));
    diesel::insert_into(authenticator_replacements::table)
        .values((
            authenticator_replacements::user_id.eq(user_id),
            authenticator_replacements::secret_encrypted.eq(&sealed),
            authenticator_replacements::expires_at.eq(expires_at),
        ))
        .on_conflict(authenticator_replacements::user_id)
        .do_update()
        .set((
            authenticator_replacements::secret_encrypted.eq(&sealed),
            authenticator_replacements::expires_at.eq(expires_at),
            authenticator_replacements::created_at.eq(Utc::now()),
        ))
        .execute(connection)
        .await
        .map(drop)
}

/// Why the person's new app's secret could not be read.
#[derive(Debug, thiserror::Error)]
pub enum FindError {
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    /// The key changed, or the stored bytes were altered or copied from another row.
    #[error(transparent)]
    Sealed(#[from] OpenError),
}

/// The secret of the person's new app, while it has not expired at `now`.
///
/// # Errors
/// Returns [`FindError`] when the database fails or the secret does not open.
pub async fn find_secret(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    user_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Option<SecretSlice<u8>>, FindError> {
    let sealed: Option<Vec<u8>> = authenticator_replacements::table
        .find(user_id)
        .filter(authenticator_replacements::expires_at.gt(now))
        .select(authenticator_replacements::secret_encrypted)
        .first(connection)
        .await
        .optional()?;
    let Some(sealed) = sealed else {
        return Ok(None);
    };
    Ok(Some(cipher.open(&sealed, &secret_context(user_id))?))
}

/// Forgets the person's new app, once it is in use. Nothing to forget is not an error.
///
/// # Errors
/// Propagates any database error.
pub async fn clear(connection: &mut AsyncPgConnection, user_id: Uuid) -> QueryResult<()> {
    diesel::delete(authenticator_replacements::table.find(user_id))
        .execute(connection)
        .await
        .map(drop)
}

#[cfg(test)]
mod tests {
    use diesel_async::AsyncConnection;
    use secrecy::{ExposeSecret, SecretString};

    use super::*;
    use crate::database::migrations::{self, MigrationCommand};
    use crate::models::user::{self, Profile};
    use crate::test_support::empty_database;

    fn cipher() -> Cipher {
        Cipher::from_base64_key(&SecretString::from(
            "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=",
        ))
        .expect("a 32-byte key")
    }

    /// A migrated database holding two people, and their ids.
    async fn database_with_two_people() -> (AsyncPgConnection, Uuid, Uuid) {
        let url = empty_database().await;
        migrations::execute(url.clone(), MigrationCommand::Run)
            .await
            .expect("migrations apply");
        let mut connection = AsyncPgConnection::establish(url.expose_secret())
            .await
            .expect("connects");
        let ada = Profile::new(Uuid::now_v7(), "ada@example.com", "Ada Lovelace");
        let grace = Profile::new(Uuid::now_v7(), "grace@example.com", "Grace Hopper");
        let ada = user::provision(&mut connection, &ada)
            .await
            .expect("provisions");
        let grace = user::provision(&mut connection, &grace)
            .await
            .expect("provisions");
        (connection, ada.id, grace.id)
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_new_app_is_held_replaced_and_cleared() {
        let (mut connection, ada, _) = database_with_two_people().await;
        let cipher = cipher();
        let now = Utc::now();
        let later = now + chrono::Duration::minutes(15);

        start(&mut connection, &cipher, ada, b"first secret", later)
            .await
            .expect("starts");
        start(&mut connection, &cipher, ada, b"second secret", later)
            .await
            .expect("starts again");
        let secret = find_secret(&mut connection, &cipher, ada, now)
            .await
            .expect("reads")
            .expect("one is waiting");
        assert_eq!(secret.expose_secret(), b"second secret");

        clear(&mut connection, ada).await.expect("clears");
        clear(&mut connection, ada)
            .await
            .expect("clearing nothing is fine");
        assert!(
            find_secret(&mut connection, &cipher, ada, now)
                .await
                .expect("reads")
                .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_expired_app_is_never_read_and_goes_at_the_next_start() {
        let (mut connection, ada, grace) = database_with_two_people().await;
        let cipher = cipher();
        let now = Utc::now();

        start(
            &mut connection,
            &cipher,
            ada,
            b"ada's secret",
            now - chrono::Duration::seconds(1),
        )
        .await
        .expect("starts");
        assert!(
            find_secret(&mut connection, &cipher, ada, now)
                .await
                .expect("reads")
                .is_none(),
            "an expired app is refused"
        );

        start(
            &mut connection,
            &cipher,
            grace,
            b"grace's secret",
            now + chrono::Duration::minutes(15),
        )
        .await
        .expect("starts");
        let rows: i64 = authenticator_replacements::table
            .count()
            .get_result(&mut connection)
            .await
            .expect("counts");
        assert_eq!(rows, 1, "Grace's start removed Ada's expired row");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_secret_copied_to_another_person_refuses_to_open() {
        use diesel::sql_types::Uuid as SqlUuid;

        let (mut connection, ada, grace) = database_with_two_people().await;
        let cipher = cipher();
        let now = Utc::now();
        let later = now + chrono::Duration::minutes(15);
        start(&mut connection, &cipher, ada, b"ada's secret", later)
            .await
            .expect("starts");
        start(&mut connection, &cipher, grace, b"grace's secret", later)
            .await
            .expect("starts");

        diesel::sql_query(
            "UPDATE authenticator_replacements SET secret_encrypted = \
             (SELECT secret_encrypted FROM authenticator_replacements WHERE user_id = $1) \
             WHERE user_id = $2",
        )
        .bind::<SqlUuid, _>(ada)
        .bind::<SqlUuid, _>(grace)
        .execute(&mut connection)
        .await
        .expect("copies");

        assert!(matches!(
            find_secret(&mut connection, &cipher, grace, now).await,
            Err(FindError::Sealed(_))
        ));
    }
}
