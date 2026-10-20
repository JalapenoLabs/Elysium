// Copyright © 2026 Jalapeno Labs

//! Browsers the host runs for people to sign in to their accounts in. See `crate::browser`.
//!
//! Each row is one Neko container. Its member password never exists in Postgres as
//! plaintext: it is sealed here, bound to the row's id, before the insert leaves the process.
//! A browser belongs to the person who created it, and every query here is scoped to them.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::browser_sessions;

/// A stored browser. `member_password_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = browser_sessions, check_for_backend(diesel::pg::Pg))]
pub struct BrowserSession {
    pub id: Uuid,
    pub name: String,
    /// The page the browser opens when it is created.
    pub start_url: String,
    /// The host port its media arrives on, which also names its container on the network.
    pub port: i32,
    pub member_password_encrypted: Vec<u8>,
    /// Who created it, and the only person it answers.
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Fields for a new browser, with the password still in plaintext.
#[derive(Debug)]
pub struct NewBrowserSession {
    pub id: Uuid,
    pub name: String,
    pub start_url: String,
    pub port: i32,
    pub member_password: SecretString,
    pub created_by: Uuid,
}

#[derive(Insertable)]
#[diesel(table_name = browser_sessions)]
struct BrowserSessionRow {
    id: Uuid,
    name: String,
    start_url: String,
    port: i32,
    member_password_encrypted: Vec<u8>,
    created_by: Uuid,
}

/// Associated data binding the sealed password to its row, so a ciphertext copied into
/// another row refuses to open.
fn member_password_context(id: Uuid) -> Vec<u8> {
    let mut context = b"browser_sessions.member_password:".to_vec();
    context.extend_from_slice(id.as_bytes());
    context
}

impl BrowserSession {
    /// Decrypts the password of the Neko member session the stream route relays.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or copied
    /// from another row.
    pub fn member_password(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(
            &self.member_password_encrypted,
            &member_password_context(self.id),
        )?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }
}

/// Every browser, for keeping their containers running.
///
/// # Errors
/// Propagates any database error.
pub async fn list_all(connection: &mut AsyncPgConnection) -> QueryResult<Vec<BrowserSession>> {
    browser_sessions::table
        .select(BrowserSession::as_select())
        .order(browser_sessions::port.asc())
        .load(connection)
        .await
}

/// The browsers `owner` created, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_owned(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
) -> QueryResult<Vec<BrowserSession>> {
    browser_sessions::table
        .filter(browser_sessions::created_by.eq(owner))
        .select(BrowserSession::as_select())
        .order(browser_sessions::created_at.asc())
        .load(connection)
        .await
}

/// The browser `id`, when `owner` created it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or belongs to someone else, so
/// another person's browser is indistinguishable from none.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<BrowserSession> {
    browser_sessions::table
        .filter(browser_sessions::id.eq(id))
        .filter(browser_sessions::created_by.eq(owner))
        .select(BrowserSession::as_select())
        .first(connection)
        .await
}

/// The ports browsers already hold.
///
/// # Errors
/// Propagates any database error.
pub async fn ports_in_use(connection: &mut AsyncPgConnection) -> QueryResult<Vec<i32>> {
    browser_sessions::table
        .select(browser_sessions::port)
        .load(connection)
        .await
}

/// Seals the password and stores the browser.
///
/// # Errors
/// A unique violation when another browser took the port first; any other database error.
pub async fn create(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_session: NewBrowserSession,
) -> QueryResult<BrowserSession> {
    let row = BrowserSessionRow {
        member_password_encrypted: cipher.seal(
            new_session.member_password.expose_secret().as_bytes(),
            &member_password_context(new_session.id),
        ),
        id: new_session.id,
        name: new_session.name,
        start_url: new_session.start_url,
        port: new_session.port,
        created_by: new_session.created_by,
    };
    diesel::insert_into(browser_sessions::table)
        .values(row)
        .returning(BrowserSession::as_returning())
        .get_result(connection)
        .await
}

/// Removes the browser `id`.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when there is no such row.
pub async fn delete(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<()> {
    let deleted = diesel::delete(browser_sessions::table.find(id))
        .execute(connection)
        .await?;
    if deleted == 0 {
        return Err(diesel::result::Error::NotFound);
    }
    Ok(())
}
