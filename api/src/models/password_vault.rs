// Copyright © 2026 Jalapeno Labs

//! The password vault's one row: where the account lives, its API key and master password, and
//! the collection agents may read.
//!
//! The client secret and the master password never exist in Postgres as plaintext. They are
//! sealed here before an insert or update leaves the process, each under its own associated
//! data, so neither ciphertext opens as the other. Callers hand in [`SecretString`]s and only
//! ever get one back from [`PasswordVault::client_secret`] and [`PasswordVault::master_password`].
//!
//! Credentials are only ever stored after the CLI has signed in and unlocked with them, which is
//! why [`save`] takes a [`VerifiedVault`]: every row names the account it reaches.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::password_vault;

/// Associated data for the sealed client secret.
const CLIENT_SECRET_CONTEXT: &[u8] = b"password_vault.client_secret";
/// Associated data for the sealed master password.
const MASTER_PASSWORD_CONTEXT: &[u8] = b"password_vault.master_password";

/// Where the account lives.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::PasswordServer"]
#[serde(rename_all = "kebab-case")]
pub enum PasswordServer {
    /// Bitwarden's servers in the United States, `vault.bitwarden.com`.
    BitwardenUs,
    /// Bitwarden's servers in the European Union, `vault.bitwarden.eu`.
    BitwardenEu,
    /// A server the operator runs: Vaultwarden, or Bitwarden's self-hosted edition.
    SelfHosted,
}

/// The stored vault. The `*_encrypted` columns are envelopes from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = password_vault, check_for_backend(diesel::pg::Pg))]
pub struct PasswordVault {
    pub server: PasswordServer,
    /// A self-hosted server's origin; `None` for Bitwarden's own servers.
    pub server_url: Option<String>,
    pub client_id: String,
    pub client_secret_encrypted: Vec<u8>,
    pub master_password_encrypted: Vec<u8>,
    pub account_email: String,
    pub collection_id: Option<Uuid>,
    pub collection_name: Option<String>,
    pub organization_id: Option<Uuid>,
    pub checked_at: DateTime<Utc>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// The collection agents may read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenCollection {
    pub id: Uuid,
    pub name: String,
    pub organization_id: Uuid,
}

/// Credentials the CLI has just signed in and unlocked with, and the account they reached.
#[derive(Debug)]
pub struct VerifiedVault {
    pub server: PasswordServer,
    pub server_url: Option<String>,
    pub client_id: String,
    pub client_secret: SecretString,
    pub master_password: SecretString,
    pub account_email: String,
}

#[derive(Insertable)]
#[diesel(table_name = password_vault)]
struct VaultRow<'a> {
    server: PasswordServer,
    server_url: Option<&'a str>,
    client_id: &'a str,
    client_secret_encrypted: Vec<u8>,
    master_password_encrypted: Vec<u8>,
    account_email: &'a str,
    collection_id: Option<Uuid>,
    collection_name: Option<&'a str>,
    organization_id: Option<Uuid>,
    checked_at: DateTime<Utc>,
    created_by: Uuid,
}

impl PasswordVault {
    /// The collection agents may read, when an admin has chosen one.
    pub fn collection(&self) -> Option<ChosenCollection> {
        Some(ChosenCollection {
            id: self.collection_id?,
            name: self.collection_name.clone()?,
            organization_id: self.organization_id?,
        })
    }

    /// Decrypts the API key's client secret.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered.
    pub fn client_secret(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        open_text(cipher, &self.client_secret_encrypted, CLIENT_SECRET_CONTEXT)
    }

    /// Decrypts the master password.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered.
    pub fn master_password(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        open_text(
            cipher,
            &self.master_password_encrypted,
            MASTER_PASSWORD_CONTEXT,
        )
    }
}

fn open_text(cipher: &Cipher, envelope: &[u8], context: &[u8]) -> Result<SecretString, OpenError> {
    let plaintext = cipher.open(envelope, context)?;
    let text = String::from_utf8(plaintext.expose_secret().to_vec())
        .map_err(|_utf8_error| OpenError::Inauthentic)?;
    Ok(SecretString::from(text))
}

/// The vault, if one is set up.
///
/// # Errors
/// Propagates any database error.
pub async fn find(connection: &mut AsyncPgConnection) -> QueryResult<Option<PasswordVault>> {
    password_vault::table
        .select(PasswordVault::as_select())
        .first(connection)
        .await
        .optional()
}

/// Stores `verified` as the vault, replacing whatever was there, with `collection` as the
/// collection agents may read. The creator of a replaced vault is kept.
///
/// # Errors
/// Propagates any database error.
pub async fn save(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    verified: &VerifiedVault,
    collection: Option<&ChosenCollection>,
    saved_by: Uuid,
) -> QueryResult<PasswordVault> {
    let row = VaultRow {
        server: verified.server,
        server_url: verified.server_url.as_deref(),
        client_id: &verified.client_id,
        client_secret_encrypted: cipher.seal(
            verified.client_secret.expose_secret().as_bytes(),
            CLIENT_SECRET_CONTEXT,
        ),
        master_password_encrypted: cipher.seal(
            verified.master_password.expose_secret().as_bytes(),
            MASTER_PASSWORD_CONTEXT,
        ),
        account_email: &verified.account_email,
        collection_id: collection.map(|collection| collection.id),
        collection_name: collection.map(|collection| collection.name.as_str()),
        organization_id: collection.map(|collection| collection.organization_id),
        checked_at: Utc::now(),
        created_by: saved_by,
    };

    diesel::insert_into(password_vault::table)
        .values(row)
        .on_conflict(password_vault::singleton)
        .do_update()
        .set((
            password_vault::server.eq(excluded(password_vault::server)),
            password_vault::server_url.eq(excluded(password_vault::server_url)),
            password_vault::client_id.eq(excluded(password_vault::client_id)),
            password_vault::client_secret_encrypted
                .eq(excluded(password_vault::client_secret_encrypted)),
            password_vault::master_password_encrypted
                .eq(excluded(password_vault::master_password_encrypted)),
            password_vault::account_email.eq(excluded(password_vault::account_email)),
            password_vault::collection_id.eq(excluded(password_vault::collection_id)),
            password_vault::collection_name.eq(excluded(password_vault::collection_name)),
            password_vault::organization_id.eq(excluded(password_vault::organization_id)),
            password_vault::checked_at.eq(excluded(password_vault::checked_at)),
        ))
        .returning(PasswordVault::as_returning())
        .get_result(connection)
        .await
}

/// Sets the collection agents may read, or clears it with `None`.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no vault is set up, and propagates any
/// other database error.
pub async fn choose_collection(
    connection: &mut AsyncPgConnection,
    collection: Option<&ChosenCollection>,
) -> QueryResult<PasswordVault> {
    diesel::update(password_vault::table)
        .set((
            password_vault::collection_id.eq(collection.map(|collection| collection.id)),
            password_vault::collection_name
                .eq(collection.map(|collection| collection.name.as_str())),
            password_vault::organization_id
                .eq(collection.map(|collection| collection.organization_id)),
        ))
        .returning(PasswordVault::as_returning())
        .get_result(connection)
        .await
}

/// Removes the vault. Returns whether there was one.
///
/// # Errors
/// Propagates any database error.
pub async fn delete(connection: &mut AsyncPgConnection) -> QueryResult<bool> {
    let removed = diesel::delete(password_vault::table)
        .execute(connection)
        .await?;
    Ok(removed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::generate_key;
    use crate::models::user::{ELYSIA_USER_ID, SYSTEM_USER_ID};
    use crate::test_support::migrated_database;

    fn cipher() -> Cipher {
        Cipher::from_base64_key(&SecretString::from(generate_key())).expect("a key")
    }

    fn verified(email: &str) -> VerifiedVault {
        VerifiedVault {
            server: PasswordServer::SelfHosted,
            server_url: Some("https://vault.example.com".to_owned()),
            client_id: "user.6c2b0d8e-2f9e-4c52-9a8e-0e3c1f6e8d11".to_owned(),
            client_secret: SecretString::from("client-secret-value"),
            master_password: SecretString::from("correct horse battery staple"),
            account_email: email.to_owned(),
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_vault_is_one_row_with_its_secrets_sealed() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        assert!(find(&mut connection).await.expect("a read").is_none());

        let collection = ChosenCollection {
            id: Uuid::now_v7(),
            name: "Elysia".to_owned(),
            organization_id: Uuid::now_v7(),
        };
        let saved = save(
            &mut connection,
            &cipher,
            &verified("elysia@example.com"),
            Some(&collection),
            SYSTEM_USER_ID,
        )
        .await
        .expect("saved");
        assert_eq!(saved.collection(), Some(collection));
        assert_eq!(
            saved
                .master_password(&cipher)
                .expect("opens")
                .expose_secret(),
            "correct horse battery staple"
        );
        assert_eq!(
            saved.client_secret(&cipher).expect("opens").expose_secret(),
            "client-secret-value"
        );
        assert!(
            !saved
                .master_password_encrypted
                .windows(8)
                .any(|window| window == b"correct "),
            "the master password is never stored in the clear"
        );

        let replaced = save(
            &mut connection,
            &cipher,
            &verified("other@example.com"),
            None,
            ELYSIA_USER_ID,
        )
        .await
        .expect("replaced");
        assert_eq!(replaced.account_email, "other@example.com");
        assert_eq!(replaced.collection(), None);
        assert_eq!(replaced.created_by, SYSTEM_USER_ID, "the creator is kept");

        assert!(delete(&mut connection).await.expect("deleted"));
        assert!(!delete(&mut connection).await.expect("nothing left"));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_ciphertext_opens_only_as_what_it_was_sealed_as() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let mut saved = save(
            &mut connection,
            &cipher,
            &verified("elysia@example.com"),
            None,
            SYSTEM_USER_ID,
        )
        .await
        .expect("saved");
        saved.master_password_encrypted = saved.client_secret_encrypted.clone();
        saved.master_password(&cipher).expect_err("refused");
    }
}
