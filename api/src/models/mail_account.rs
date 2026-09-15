// Copyright © 2026 Jalapeno Labs

//! Connected mailboxes.
//!
//! The credential never exists in Postgres as plaintext. It is sealed here, bound to
//! the row's id, before the insert or update leaves the process: a refresh token for
//! OAuth accounts, a password for self-hosted ones. Callers hand in a
//! [`SecretString`] and only ever get one back from [`MailAccount::credential`].

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::mail_accounts;

/// Who hosts a mailbox, which decides its servers and how it authenticates.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::MailAccountKind"]
#[serde(rename_all = "kebab-case")]
pub enum MailAccountKind {
    Gmail,
    Outlook,
    SelfHosted,
}

/// A stored mailbox. `credential_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = mail_accounts, check_for_backend(diesel::pg::Pg))]
pub struct MailAccount {
    pub id: Uuid,
    pub kind: MailAccountKind,
    pub address: String,
    pub display_name: String,
    pub credential_encrypted: Vec<u8>,
    pub external_id: Option<String>,
    pub is_active: bool,
    pub last_checked_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The domain a self-hosted mailbox lives on; `None` for OAuth accounts.
    pub mail_domain_id: Option<Uuid>,
}

/// Fields for a new mailbox, with the credential still in plaintext.
#[derive(Debug)]
pub struct NewMailAccount {
    pub kind: MailAccountKind,
    /// Lowercased before it is stored.
    pub address: String,
    pub display_name: String,
    pub credential: SecretString,
    /// Stalwart's account id; required for, and only for, self-hosted mailboxes.
    pub external_id: Option<String>,
    /// The mail domain; required for, and only for, self-hosted mailboxes.
    pub mail_domain_id: Option<Uuid>,
}

/// A partial update. `None` leaves a column untouched.
#[derive(Debug, Default)]
pub struct MailAccountChanges {
    pub display_name: Option<String>,
    pub is_active: Option<bool>,
}

impl MailAccountChanges {
    /// True when applying these changes would not touch any column.
    pub const fn is_empty(&self) -> bool {
        self.display_name.is_none() && self.is_active.is_none()
    }
}

#[derive(Insertable)]
#[diesel(table_name = mail_accounts)]
struct MailAccountRow<'a> {
    id: Uuid,
    kind: MailAccountKind,
    address: String,
    display_name: &'a str,
    credential_encrypted: Vec<u8>,
    external_id: Option<&'a str>,
    mail_domain_id: Option<Uuid>,
}

#[derive(AsChangeset)]
#[diesel(table_name = mail_accounts)]
struct MailAccountChangeset<'a> {
    display_name: Option<&'a str>,
    is_active: Option<bool>,
}

/// Associated data binding a sealed credential to its row, so a ciphertext copied into
/// another row refuses to open.
fn credential_context(id: Uuid) -> Vec<u8> {
    let mut context = b"mail_accounts.credential:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl MailAccount {
    /// Decrypts this mailbox's credential.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or
    /// copied from another row.
    pub fn credential(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(&self.credential_encrypted, &credential_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every mailbox, alphabetically by address.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<MailAccount>> {
    mail_accounts::table
        .order(mail_accounts::address.asc())
        .select(MailAccount::as_select())
        .load(connection)
        .await
}

/// One mailbox by id.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn find(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<MailAccount> {
    mail_accounts::table
        .find(id)
        .select(MailAccount::as_select())
        .first(connection)
        .await
}

/// The mailbox with `address`, compared case-insensitively.
///
/// # Errors
/// Propagates any database error.
pub async fn find_by_address(
    connection: &mut AsyncPgConnection,
    address: &str,
) -> QueryResult<Option<MailAccount>> {
    mail_accounts::table
        .filter(mail_accounts::address.eq(address.to_lowercase()))
        .select(MailAccount::as_select())
        .first(connection)
        .await
        .optional()
}

/// Seals the credential and inserts the mailbox with a new `UUIDv7` id.
///
/// # Errors
/// Propagates database errors, including a unique violation on `address`.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_account: &NewMailAccount,
) -> QueryResult<MailAccount> {
    let id = Uuid::now_v7();
    let row = MailAccountRow {
        id,
        kind: new_account.kind,
        address: new_account.address.to_lowercase(),
        display_name: &new_account.display_name,
        credential_encrypted: cipher.seal(
            new_account.credential.expose_secret().as_bytes(),
            &credential_context(id),
        ),
        external_id: new_account.external_id.as_deref(),
        mail_domain_id: new_account.mail_domain_id,
    };

    diesel::insert_into(mail_accounts::table)
        .values(row)
        .returning(MailAccount::as_returning())
        .get_result(connection)
        .await
}

/// Applies `changes`.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id and
/// [`diesel::result::Error::QueryBuilderError`] when `changes` is empty.
pub async fn update(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    changes: &MailAccountChanges,
) -> QueryResult<MailAccount> {
    let changeset = MailAccountChangeset {
        display_name: changes.display_name.as_deref(),
        is_active: changes.is_active,
    };

    diesel::update(mail_accounts::table.find(id))
        .set(changeset)
        .returning(MailAccount::as_returning())
        .get_result(connection)
        .await
}

/// Seals a replacement credential: a reconnected account, or a refresh token the
/// provider rotated. A new credential says nothing about the old one's failures, so
/// the recorded error is cleared.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id.
pub async fn replace_credential(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    id: Uuid,
    credential: &SecretString,
) -> QueryResult<MailAccount> {
    diesel::update(mail_accounts::table.find(id))
        .set((
            mail_accounts::credential_encrypted.eq(cipher.seal(
                credential.expose_secret().as_bytes(),
                &credential_context(id),
            )),
            mail_accounts::last_error.eq(None::<String>),
        ))
        .returning(MailAccount::as_returning())
        .get_result(connection)
        .await
}

/// Records the outcome of a connection check: `None` for success, else the reason.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown id.
pub async fn record_check(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    error: Option<&str>,
) -> QueryResult<MailAccount> {
    diesel::update(mail_accounts::table.find(id))
        .set((
            mail_accounts::last_checked_at.eq(Utc::now()),
            mail_accounts::last_error.eq(error),
        ))
        .returning(MailAccount::as_returning())
        .get_result(connection)
        .await
}

/// Deletes one mailbox row. Removing a self-hosted mailbox from Stalwart is the
/// caller's job, done first.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when no row has that id.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(mail_accounts::table.find(id))
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
    use crate::errors::ApiError;
    use crate::test_support::{cipher, migrated_database};

    fn gmail(address: &str) -> NewMailAccount {
        NewMailAccount {
            kind: MailAccountKind::Gmail,
            address: address.to_owned(),
            display_name: "Someone".to_owned(),
            credential: SecretString::from(format!("refresh-{address}")),
            external_id: None,
            mail_domain_id: None,
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn credentials_are_sealed_and_addresses_are_unique_regardless_of_case() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let created = create(&mut connection, &cipher, &gmail("Someone@Gmail.com"))
            .await
            .expect("insert");
        assert_eq!(created.address, "someone@gmail.com");
        assert_eq!(
            created
                .credential(&cipher)
                .expect("decrypts")
                .expose_secret(),
            "refresh-Someone@Gmail.com"
        );
        let plaintext = b"refresh-";
        assert!(
            !created
                .credential_encrypted
                .windows(plaintext.len())
                .any(|window| window == plaintext)
        );

        let found = find_by_address(&mut connection, "SOMEONE@gmail.com")
            .await
            .expect("select")
            .expect("found");
        assert_eq!(found.id, created.id);

        let duplicate = create(&mut connection, &cipher, &gmail("someone@GMAIL.com"))
            .await
            .unwrap_err();
        assert!(matches!(ApiError::from(duplicate), ApiError::Conflict(_)));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_new_credential_clears_the_recorded_failure() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let account = create(&mut connection, &cipher, &gmail("rotating@gmail.com"))
            .await
            .expect("insert");

        let failed = record_check(&mut connection, account.id, Some("invalid_grant"))
            .await
            .expect("record");
        assert_eq!(failed.last_error.as_deref(), Some("invalid_grant"));
        assert!(failed.last_checked_at.is_some());

        let replaced = replace_credential(
            &mut connection,
            &cipher,
            account.id,
            &SecretString::from("refresh-rotated"),
        )
        .await
        .expect("replace");
        assert_eq!(replaced.last_error, None);
        assert_eq!(
            replaced
                .credential(&cipher)
                .expect("decrypts")
                .expose_secret(),
            "refresh-rotated"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn only_self_hosted_mailboxes_carry_a_stalwart_id() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();

        let mut gmail_with_id = gmail("wrong@gmail.com");
        gmail_with_id.external_id = Some("b".to_owned());
        create(&mut connection, &cipher, &gmail_with_id)
            .await
            .expect_err("an OAuth mailbox has no Stalwart id");

        let self_hosted_without_id = NewMailAccount {
            kind: MailAccountKind::SelfHosted,
            ..gmail("agent@elysium.local")
        };
        create(&mut connection, &cipher, &self_hosted_without_id)
            .await
            .expect_err("a self-hosted mailbox needs its Stalwart id");
    }
}
