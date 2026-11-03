// Copyright © 2026 Jalapeno Labs

//! Token accounts: a person's account with a provider whose whole sign-in is one API key, such
//! as Fireflies. See `docs/connectors.md`.
//!
//! The key is sealed bound to the row's id, so a ciphertext copied into another row refuses to
//! open, and it is never returned. What the provider answered about the key the last time it
//! was checked is kept as `identity`, such as the address it belongs to. An account belongs to
//! whoever created it; removing it removes the connectors it feeds.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::token_accounts;

/// The label a key is sealed under, beside the row's id. It only has to stay the same for
/// every stored key to keep opening, so it is never changed; the keys carried into this table
/// were sealed under it.
const CREDENTIAL_PURPOSE: &[u8] = b"connector_accounts.credential:";

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = token_accounts, check_for_backend(diesel::pg::Pg))]
pub struct TokenAccount {
    pub id: Uuid,
    /// The provider the key is for, such as `fireflies`.
    pub kind: String,
    pub name: String,
    /// The key, an envelope from [`Cipher::seal`]. Read it with [`TokenAccount::credential`].
    pub credential_encrypted: Vec<u8>,
    /// Who the provider says the key belongs to, as of the last check.
    pub identity: String,
    pub checked_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The instance the key was issued on, for a provider whose accounts each name one, such as
    /// a YouTrack Cloud site: an https origin, validated before it is stored.
    pub site_url: Option<String>,
}

impl TokenAccount {
    /// The account's key.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the encryption key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn credential(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(&self.credential_encrypted, &credential_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Associated data binding a sealed key to its row.
fn credential_context(id: Uuid) -> Vec<u8> {
    let mut context = CREDENTIAL_PURPOSE.to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

#[derive(Debug, Insertable)]
#[diesel(table_name = token_accounts)]
pub struct NewTokenAccount {
    pub id: Uuid,
    pub kind: String,
    pub name: String,
    pub credential_encrypted: Vec<u8>,
    pub identity: String,
    pub checked_at: Option<DateTime<Utc>>,
    pub created_by: Uuid,
    /// See [`TokenAccount::site_url`]; set it after [`NewTokenAccount::new`] for a provider
    /// that has one.
    pub site_url: Option<String>,
}

impl NewTokenAccount {
    /// An account for `kind` that signs in with `credential`, sealed here under the new row's
    /// id, which the provider said belongs to `identity` just now.
    pub fn new(
        cipher: &Cipher,
        kind: &str,
        name: &str,
        credential: &SecretString,
        identity: &str,
        created_by: Uuid,
    ) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            kind: kind.to_owned(),
            name: name.to_owned(),
            credential_encrypted: cipher.seal(
                credential.expose_secret().as_bytes(),
                &credential_context(id),
            ),
            identity: identity.chars().take(512).collect(),
            checked_at: Some(Utc::now()),
            created_by,
            site_url: None,
        }
    }
}

/// The accounts `owner` created, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_owned(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
) -> QueryResult<Vec<TokenAccount>> {
    token_accounts::table
        .filter(token_accounts::created_by.eq(owner))
        .select(TokenAccount::as_select())
        .order(token_accounts::created_at.asc())
        .load(connection)
        .await
}

/// The account `id`, when `owner` created it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or belongs to someone else.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<TokenAccount> {
    token_accounts::table
        .filter(token_accounts::id.eq(id))
        .filter(token_accounts::created_by.eq(owner))
        .select(TokenAccount::as_select())
        .first(connection)
        .await
}

/// The account `id`, whoever created it, for a loop reading on its owner's behalf.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<TokenAccount> {
    token_accounts::table
        .find(id)
        .select(TokenAccount::as_select())
        .first(connection)
        .await
}

/// Stores a new account.
///
/// # Errors
/// Propagates any database error.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_account: NewTokenAccount,
) -> QueryResult<TokenAccount> {
    diesel::insert_into(token_accounts::table)
        .values(new_account)
        .returning(TokenAccount::as_returning())
        .get_result(connection)
        .await
}

/// Removes the account `id` and every connector it feeds.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(token_accounts::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_opens_only_on_its_own_row() {
        let cipher = crate::test_support::cipher();
        let new_account = NewTokenAccount::new(
            &cipher,
            "fireflies",
            "Fireflies",
            &SecretString::from("key-123"),
            "ada@example.com",
            Uuid::now_v7(),
        );
        let account = TokenAccount {
            id: new_account.id,
            kind: new_account.kind,
            name: new_account.name,
            credential_encrypted: new_account.credential_encrypted,
            identity: new_account.identity,
            checked_at: new_account.checked_at,
            created_by: new_account.created_by,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            site_url: new_account.site_url,
        };
        assert_eq!(
            account
                .credential(&cipher)
                .expect("it opens")
                .expose_secret(),
            "key-123"
        );

        let copied = TokenAccount {
            id: Uuid::now_v7(),
            ..account
        };
        copied
            .credential(&cipher)
            .expect_err("a key copied to another row does not open");
    }
}
