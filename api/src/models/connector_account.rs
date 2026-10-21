// Copyright © 2026 Jalapeno Labs

//! Services Elysium reads as one of its people. See `crate::connectors`.
//!
//! An account is a connector (`kind`) signed in one of two ways: through a browser the person
//! signed in to the service in, or with a credential they gave Elysium, such as an API key.
//! Exactly one of the two is set, which the `connector_accounts_one_sign_in` constraint
//! holds. The credential is sealed bound to the row's id, so a ciphertext copied into
//! another row refuses to open. An account belongs to whoever created it; removing its
//! browser removes it.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use serde_json::Value;
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::{browser_sessions, connector_accounts};

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = connector_accounts, check_for_backend(diesel::pg::Pg))]
pub struct ConnectorAccount {
    pub id: Uuid,
    /// Which connector reads it; see `crate::connectors::ConnectorKind`.
    pub kind: String,
    /// The browser it reads through, for a connector that signs in through one.
    pub browser_session_id: Option<Uuid>,
    /// `signed_in`, `signed_out`, or `unknown`, as last read.
    pub sign_in_state: String,
    /// Where the last read left off, in the connector's own terms.
    pub cursor: Value,
    pub last_polled_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub is_active: bool,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// The credential it signs in with, an envelope from [`Cipher::seal`], for a connector
    /// that signs in without a browser. Read it with [`ConnectorAccount::credential`].
    pub credential_encrypted: Option<Vec<u8>>,
    /// Where a connector that keeps files saves them, such as meeting recordings.
    pub storage_location_id: Option<Uuid>,
}

impl ConnectorAccount {
    /// The credential the account signs in with, or `None` for one that reads through a
    /// browser.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or copied
    /// from another row.
    pub fn credential(&self, cipher: &Cipher) -> Result<Option<SecretString>, OpenError> {
        let Some(envelope) = &self.credential_encrypted else {
            return Ok(None);
        };
        let plaintext = cipher.open(envelope, &credential_context(self.id))?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(Some(SecretString::from(text)))
    }
}

/// Associated data binding a sealed credential to its row.
fn credential_context(id: Uuid) -> Vec<u8> {
    let mut context = b"connector_accounts.credential:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

#[derive(Debug, Insertable)]
#[diesel(table_name = connector_accounts)]
pub struct NewConnectorAccount {
    pub id: Uuid,
    pub kind: String,
    pub browser_session_id: Option<Uuid>,
    pub credential_encrypted: Option<Vec<u8>>,
    pub storage_location_id: Option<Uuid>,
    pub created_by: Uuid,
}

impl NewConnectorAccount {
    /// An account that reads through the browser `browser_session_id`.
    pub fn through_browser(kind: &str, browser_session_id: Uuid, created_by: Uuid) -> Self {
        Self {
            id: Uuid::now_v7(),
            kind: kind.to_owned(),
            browser_session_id: Some(browser_session_id),
            credential_encrypted: None,
            storage_location_id: None,
            created_by,
        }
    }

    /// An account that signs in with `credential`, sealed here under the new row's id, and
    /// keeps its files in `storage_location_id`.
    pub fn with_credential(
        cipher: &Cipher,
        kind: &str,
        credential: &SecretString,
        storage_location_id: Option<Uuid>,
        created_by: Uuid,
    ) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            kind: kind.to_owned(),
            browser_session_id: None,
            credential_encrypted: Some(cipher.seal(
                credential.expose_secret().as_bytes(),
                &credential_context(id),
            )),
            storage_location_id,
            created_by,
        }
    }
}

/// What one read of an account recorded about it.
#[derive(Debug, AsChangeset)]
#[diesel(table_name = connector_accounts, treat_none_as_null = true)]
pub struct PollOutcome {
    pub sign_in_state: String,
    pub cursor: Value,
    pub last_polled_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

/// Every active account of one of `kinds`, oldest first, for the loop that reads them.
///
/// # Errors
/// Propagates any database error.
pub async fn list_active(
    connection: &mut AsyncPgConnection,
    kinds: &[&str],
) -> QueryResult<Vec<ConnectorAccount>> {
    connector_accounts::table
        .filter(connector_accounts::is_active.eq(true))
        .filter(connector_accounts::kind.eq_any(kinds))
        .select(ConnectorAccount::as_select())
        .order(connector_accounts::created_at.asc())
        .load(connection)
        .await
}

/// The accounts `owner` created, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_owned(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
) -> QueryResult<Vec<ConnectorAccount>> {
    connector_accounts::table
        .filter(connector_accounts::created_by.eq(owner))
        .select(ConnectorAccount::as_select())
        .order(connector_accounts::created_at.asc())
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
) -> QueryResult<ConnectorAccount> {
    connector_accounts::table
        .filter(connector_accounts::id.eq(id))
        .filter(connector_accounts::created_by.eq(owner))
        .select(ConnectorAccount::as_select())
        .first(connection)
        .await
}

/// Stores a new account. One that reads through a browser needs its owner to own that
/// browser.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when the browser does not exist or belongs to someone
/// else; a unique violation when the browser already has a connector; a foreign key
/// violation for a storage location that does not exist.
pub async fn create(
    connection: &mut AsyncPgConnection,
    new_account: NewConnectorAccount,
) -> QueryResult<ConnectorAccount> {
    if let Some(browser_session_id) = new_account.browser_session_id {
        let owns_browser: bool = diesel::select(diesel::dsl::exists(
            browser_sessions::table
                .filter(browser_sessions::id.eq(browser_session_id))
                .filter(browser_sessions::created_by.eq(new_account.created_by)),
        ))
        .get_result(connection)
        .await?;
        if !owns_browser {
            return Err(diesel::result::Error::NotFound);
        }
    }

    diesel::insert_into(connector_accounts::table)
        .values(new_account)
        .returning(ConnectorAccount::as_returning())
        .get_result(connection)
        .await
}

/// Records what a read found.
///
/// # Errors
/// Propagates any database error.
pub async fn record_poll(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    outcome: PollOutcome,
) -> QueryResult<ConnectorAccount> {
    diesel::update(connector_accounts::table.find(id))
        .set(outcome)
        .returning(ConnectorAccount::as_returning())
        .get_result(connection)
        .await
}

/// Removes the account `id`. Its records stay on the timeline.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(connector_accounts::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}
