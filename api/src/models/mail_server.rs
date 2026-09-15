// Copyright © 2026 Jalapeno Labs

//! The bundled mail server's administrator credential.
//!
//! Stalwart issues the administrator when its setup completes, and nothing else ever
//! knows the secret: it is sealed here, bound to the row's id, before it reaches
//! Postgres. The table holds at most one row, and setting the server up again replaces
//! it, since a new setup means Stalwart started over with a new administrator.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::mail_servers;

/// The stored administrator. `admin_secret_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = mail_servers, check_for_backend(diesel::pg::Pg))]
pub struct MailServer {
    pub id: Uuid,
    pub domain: String,
    pub admin_username: String,
    pub admin_secret_encrypted: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

/// A freshly issued administrator, with the secret still in plaintext.
#[derive(Debug)]
pub struct NewMailServer {
    /// Lowercased before it is stored.
    pub domain: String,
    pub admin_username: String,
    pub admin_secret: SecretString,
}

#[derive(Insertable)]
#[diesel(table_name = mail_servers)]
struct MailServerRow<'a> {
    id: Uuid,
    domain: String,
    admin_username: &'a str,
    admin_secret_encrypted: Vec<u8>,
}

/// Associated data binding the sealed secret to its row, so a ciphertext copied into
/// another row refuses to open.
fn admin_secret_context(id: Uuid) -> Vec<u8> {
    let mut context = b"mail_servers.admin_secret:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl MailServer {
    /// Decrypts the administrator's secret.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn admin_secret(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext =
            cipher.open(&self.admin_secret_encrypted, &admin_secret_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// The mail server, or `None` before its setup has completed.
///
/// # Errors
/// Propagates any database error.
pub async fn find(connection: &mut AsyncPgConnection) -> QueryResult<Option<MailServer>> {
    mail_servers::table
        .select(MailServer::as_select())
        .first(connection)
        .await
        .optional()
}

/// Seals the secret and stores the administrator in place of any previous one, in one
/// transaction so the table is never left empty by a failed insert.
///
/// # Errors
/// Propagates database errors.
pub async fn replace(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_server: &NewMailServer,
) -> QueryResult<MailServer> {
    let id = Uuid::now_v7();
    let row = MailServerRow {
        id,
        domain: new_server.domain.to_lowercase(),
        admin_username: &new_server.admin_username,
        admin_secret_encrypted: cipher.seal(
            new_server.admin_secret.expose_secret().as_bytes(),
            &admin_secret_context(id),
        ),
    };

    connection
        .transaction(async move |connection| {
            diesel::delete(mail_servers::table)
                .execute(connection)
                .await?;
            diesel::insert_into(mail_servers::table)
                .values(row)
                .returning(MailServer::as_returning())
                .get_result(connection)
                .await
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{cipher, migrated_database};

    fn administrator(domain: &str, secret: &str) -> NewMailServer {
        NewMailServer {
            domain: domain.to_owned(),
            admin_username: format!("admin@{domain}"),
            admin_secret: SecretString::from(secret.to_owned()),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn setting_up_again_replaces_the_administrator() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        assert!(find(&mut connection).await.expect("select").is_none());

        let first = replace(
            &mut connection,
            &cipher,
            &administrator("Elysium.Local", "first-secret"),
        )
        .await
        .expect("insert");
        assert_eq!(first.domain, "elysium.local");
        let plaintext = b"first-secret";
        assert!(
            !first
                .admin_secret_encrypted
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );

        let second = replace(
            &mut connection,
            &cipher,
            &administrator("mail.example", "second-secret"),
        )
        .await
        .expect("replace");
        let stored = find(&mut connection)
            .await
            .expect("select")
            .expect("a server is set up");
        assert_eq!(stored.id, second.id);
        assert_eq!(
            stored
                .admin_secret(&cipher)
                .expect("decrypts")
                .expose_secret(),
            "second-secret"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_table_never_holds_a_second_server() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        replace(
            &mut connection,
            &cipher,
            &administrator("elysium.local", "secret"),
        )
        .await
        .expect("insert");

        let id = Uuid::now_v7();
        let second = diesel::insert_into(mail_servers::table)
            .values(MailServerRow {
                id,
                domain: "other.local".to_owned(),
                admin_username: "admin@other.local",
                admin_secret_encrypted: cipher.seal(b"secret", &admin_secret_context(id)),
            })
            .execute(&mut connection)
            .await;
        assert!(second.is_err(), "the singleton index refuses a second row");
    }
}
