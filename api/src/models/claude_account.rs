// Copyright © 2026 Jalapeno Labs

//! Claude accounts signed in through OAuth: the refresh side of a `claude_account` credential.
//!
//! The credential itself is an ordinary [`Llm`] row of type [`LlmType::ClaudeAccount`], whose
//! sealed token is the current access token, the one bearer a turn is handed. Its
//! `claude_accounts` row holds what keeps that token fresh: the refresh token (sealed here,
//! bound to the credential's id), both expiries, whose account it is, and the latest usage.
//!
//! Every write that rotates a refresh token runs under [`lock`], a row lock taken in the same
//! transaction that reads the old token, so two refreshes can never send the same token.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use secrecy::{ExposeSecret, SecretString};
use uuid::Uuid;

use crate::crypto::{Cipher, OpenError};
use crate::database::schema::claude_accounts;
use crate::models::llm::{self, Llm, LlmChanges, LlmType, NewLlm};

/// `sign_in_state` while Anthropic accepts the refresh token.
pub const SIGNED_IN: &str = "signed_in";
/// `sign_in_state` once Anthropic refused the refresh token. Only a new sign-in clears it.
pub const EXPIRED: &str = "expired";

/// One signed-in account. `refresh_token_encrypted` is an envelope from [`Cipher::seal`].
#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = claude_accounts, primary_key(llm_id), check_for_backend(diesel::pg::Pg))]
pub struct ClaudeAccount {
    pub llm_id: Uuid,
    pub account_uuid: Uuid,
    pub email: String,
    pub organization_uuid: Option<Uuid>,
    pub organization_name: Option<String>,
    pub subscription_type: Option<String>,
    pub refresh_token_encrypted: Vec<u8>,
    pub access_token_expires_at: DateTime<Utc>,
    pub refresh_token_expires_at: Option<DateTime<Utc>>,
    pub sign_in_state: String,
    pub last_refreshed_at: Option<DateTime<Utc>>,
    pub last_refresh_error: Option<String>,
    pub five_hour_utilization: Option<f64>,
    pub five_hour_resets_at: Option<DateTime<Utc>>,
    pub seven_day_utilization: Option<f64>,
    pub seven_day_resets_at: Option<DateTime<Utc>>,
    pub usage_read_at: Option<DateTime<Utc>>,
    pub usage_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ClaudeAccount {
    /// Whether Anthropic still accepts this account's refresh token.
    pub fn is_signed_in(&self) -> bool {
        self.sign_in_state == SIGNED_IN
    }

    /// Decrypts the refresh token.
    ///
    /// # Errors
    /// Returns [`OpenError`] if the key changed or the stored bytes were altered or copied from
    /// another row.
    pub fn refresh_token(&self, cipher: &Cipher) -> Result<SecretString, OpenError> {
        let plaintext = cipher.open(
            &self.refresh_token_encrypted,
            &refresh_token_context(self.llm_id),
        )?;
        let text = String::from_utf8(plaintext.expose_secret().to_vec())
            .map_err(|_utf8_error| OpenError::Inauthentic)?;
        Ok(SecretString::from(text))
    }

    /// The latest usage Anthropic reported, as one value.
    pub fn usage(&self) -> UsageWindows {
        UsageWindows {
            five_hour: self.five_hour_utilization.map(|utilization| UsageWindow {
                utilization,
                resets_at: self.five_hour_resets_at,
            }),
            seven_day: self.seven_day_utilization.map(|utilization| UsageWindow {
                utilization,
                resets_at: self.seven_day_resets_at,
            }),
        }
    }
}

/// One usage window: how much of it is used, as a percentage, and when it resets.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UsageWindow {
    /// From 0; 100 means the window is spent. Anthropic can report past 100.
    pub utilization: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

/// The windows Claude subscriptions are limited by. A window Anthropic did not report is `None`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UsageWindows {
    /// The rolling five-hour window.
    pub five_hour: Option<UsageWindow>,
    /// The weekly window Claude Code draws on.
    pub seven_day: Option<UsageWindow>,
}

/// A fresh token pair, from a sign-in or a refresh.
#[derive(Debug)]
pub struct Tokens {
    pub access_token: SecretString,
    pub access_token_expires_at: DateTime<Utc>,
    pub refresh_token: SecretString,
    pub refresh_token_expires_at: Option<DateTime<Utc>>,
}

/// Whose account a sign-in proved, as Anthropic's profile reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identity {
    pub account_uuid: Uuid,
    pub email: String,
    pub organization_uuid: Option<Uuid>,
    pub organization_name: Option<String>,
}

/// Everything a finished sign-in produced.
#[derive(Debug)]
pub struct SignedIn {
    pub tokens: Tokens,
    pub identity: Identity,
    pub subscription_type: Option<String>,
}

/// What [`save_sign_in`] did.
#[derive(Debug)]
pub struct SavedSignIn {
    pub llm: Llm,
    pub account: ClaudeAccount,
    /// False when the account was already stored, so its tokens were replaced.
    pub is_new: bool,
}

#[derive(Insertable)]
#[diesel(table_name = claude_accounts)]
struct AccountRow<'a> {
    llm_id: Uuid,
    account_uuid: Uuid,
    email: &'a str,
    organization_uuid: Option<Uuid>,
    organization_name: Option<&'a str>,
    subscription_type: Option<&'a str>,
    refresh_token_encrypted: Vec<u8>,
    access_token_expires_at: DateTime<Utc>,
    refresh_token_expires_at: Option<DateTime<Utc>>,
}

/// Associated data binding a sealed refresh token to its credential, so a ciphertext copied
/// into another row refuses to open.
fn refresh_token_context(llm_id: Uuid) -> Vec<u8> {
    let mut context = b"claude_accounts.refresh_token:".to_vec();
    context.extend_from_slice(llm_id.as_bytes());
    context
}

/// Every account, keyed by nothing in particular; callers match them to credentials by
/// `llm_id`.
///
/// # Errors
/// Propagates any database error.
pub async fn list(connection: &mut AsyncPgConnection) -> QueryResult<Vec<ClaudeAccount>> {
    claude_accounts::table
        .order(claude_accounts::created_at.asc())
        .select(ClaudeAccount::as_select())
        .load(connection)
        .await
}

/// The account of one credential, if it is a Claude account.
///
/// # Errors
/// Propagates any database error.
pub async fn find(
    connection: &mut AsyncPgConnection,
    llm_id: Uuid,
) -> QueryResult<Option<ClaudeAccount>> {
    claude_accounts::table
        .find(llm_id)
        .select(ClaudeAccount::as_select())
        .first(connection)
        .await
        .optional()
}

/// Locks an account's row until the surrounding transaction ends, and reads it. A refresh holds
/// this lock while it trades the refresh token, so a second refresh waits and then reads the
/// token the first one stored.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the account was deleted.
pub async fn lock(connection: &mut AsyncPgConnection, llm_id: Uuid) -> QueryResult<ClaudeAccount> {
    claude_accounts::table
        .find(llm_id)
        .for_update()
        .select(ClaudeAccount::as_select())
        .first(connection)
        .await
}

/// Stores a finished sign-in. A new account becomes a new `claude_account` credential named
/// `name` at `priority`; an account already stored keeps its credential, name, and priority,
/// and takes the new tokens, which also clears a refused sign-in.
///
/// # Errors
/// Propagates database errors, including a unique violation on a new credential's name.
pub async fn save_sign_in(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    new_credential: NewCredential<'_>,
    signed_in: &SignedIn,
) -> QueryResult<SavedSignIn> {
    connection
        .transaction(async move |connection| {
            let existing = claude_accounts::table
                .filter(claude_accounts::account_uuid.eq(signed_in.identity.account_uuid))
                .for_update()
                .select(ClaudeAccount::as_select())
                .first(connection)
                .await
                .optional()?;

            if let Some(existing) = existing {
                let llm =
                    store_tokens(connection, cipher, existing.llm_id, &signed_in.tokens).await?;
                let account = diesel::update(claude_accounts::table.find(existing.llm_id))
                    .set((
                        claude_accounts::email.eq(&signed_in.identity.email),
                        claude_accounts::organization_uuid.eq(signed_in.identity.organization_uuid),
                        claude_accounts::organization_name
                            .eq(signed_in.identity.organization_name.as_deref()),
                        claude_accounts::subscription_type
                            .eq(signed_in.subscription_type.as_deref()),
                        claude_accounts::sign_in_state.eq(SIGNED_IN),
                        claude_accounts::last_refresh_error.eq(None::<String>),
                    ))
                    .returning(ClaudeAccount::as_returning())
                    .get_result(connection)
                    .await?;
                return Ok(SavedSignIn {
                    llm,
                    account,
                    is_new: false,
                });
            }

            let llm = llm::create(
                connection,
                cipher,
                &NewLlm {
                    name: new_credential.name.to_owned(),
                    description: String::new(),
                    type_: LlmType::ClaudeAccount,
                    secret_token: SecretString::from(
                        signed_in.tokens.access_token.expose_secret().to_owned(),
                    ),
                    priority: new_credential.priority,
                    is_active: true,
                    // The access token's expiry is the account's to track; the credential
                    // itself does not expire while the keeper refreshes it.
                    expires_at: None,
                    created_by: new_credential.created_by,
                },
            )
            .await?;
            let row = AccountRow {
                llm_id: llm.id,
                account_uuid: signed_in.identity.account_uuid,
                email: &signed_in.identity.email,
                organization_uuid: signed_in.identity.organization_uuid,
                organization_name: signed_in.identity.organization_name.as_deref(),
                subscription_type: signed_in.subscription_type.as_deref(),
                refresh_token_encrypted: cipher.seal(
                    signed_in.tokens.refresh_token.expose_secret().as_bytes(),
                    &refresh_token_context(llm.id),
                ),
                access_token_expires_at: signed_in.tokens.access_token_expires_at,
                refresh_token_expires_at: signed_in.tokens.refresh_token_expires_at,
            };
            let account = diesel::insert_into(claude_accounts::table)
                .values(row)
                .returning(ClaudeAccount::as_returning())
                .get_result(connection)
                .await?;
            Ok(SavedSignIn {
                llm,
                account,
                is_new: true,
            })
        })
        .await
}

/// What a sign-in's new credential is called, where it sits in the failover stack, and who
/// created it.
#[derive(Debug, Clone, Copy)]
pub struct NewCredential<'a> {
    pub name: &'a str,
    pub priority: i32,
    pub created_by: Uuid,
}

/// Stores a refreshed token pair: the access token on the credential, everything else on the
/// account. Call it inside the transaction that holds [`lock`].
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the account was deleted.
pub async fn store_refreshed(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    llm_id: Uuid,
    tokens: &Tokens,
    now: DateTime<Utc>,
) -> QueryResult<(Llm, ClaudeAccount)> {
    let llm = store_tokens(connection, cipher, llm_id, tokens).await?;
    let account = diesel::update(claude_accounts::table.find(llm_id))
        .set((
            claude_accounts::last_refreshed_at.eq(Some(now)),
            claude_accounts::last_refresh_error.eq(None::<String>),
        ))
        .returning(ClaudeAccount::as_returning())
        .get_result(connection)
        .await?;
    Ok((llm, account))
}

/// Seals a token pair onto the credential and its account.
async fn store_tokens(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    llm_id: Uuid,
    tokens: &Tokens,
) -> QueryResult<Llm> {
    let llm = llm::update(
        connection,
        cipher,
        llm_id,
        &LlmChanges {
            secret_token: Some(SecretString::from(
                tokens.access_token.expose_secret().to_owned(),
            )),
            ..LlmChanges::default()
        },
    )
    .await?;
    diesel::update(claude_accounts::table.find(llm_id))
        .set((
            claude_accounts::refresh_token_encrypted.eq(cipher.seal(
                tokens.refresh_token.expose_secret().as_bytes(),
                &refresh_token_context(llm_id),
            )),
            claude_accounts::access_token_expires_at.eq(tokens.access_token_expires_at),
            claude_accounts::refresh_token_expires_at.eq(tokens.refresh_token_expires_at),
        ))
        .execute(connection)
        .await?;
    Ok(llm)
}

/// Marks an account's sign-in refused, with Anthropic's reason. Returns the account as stored.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the account was deleted.
pub async fn mark_expired(
    connection: &mut AsyncPgConnection,
    llm_id: Uuid,
    reason: &str,
) -> QueryResult<ClaudeAccount> {
    diesel::update(claude_accounts::table.find(llm_id))
        .set((
            claude_accounts::sign_in_state.eq(EXPIRED),
            claude_accounts::last_refresh_error.eq(Some(reason)),
        ))
        .returning(ClaudeAccount::as_returning())
        .get_result(connection)
        .await
}

/// Records why a refresh failed for a reason other than a refused token, such as Anthropic
/// being unreachable. The keeper tries again on its next pass.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the account was deleted.
pub async fn record_refresh_error(
    connection: &mut AsyncPgConnection,
    llm_id: Uuid,
    reason: &str,
) -> QueryResult<ClaudeAccount> {
    diesel::update(claude_accounts::table.find(llm_id))
        .set(claude_accounts::last_refresh_error.eq(Some(reason)))
        .returning(ClaudeAccount::as_returning())
        .get_result(connection)
        .await
}

/// Stores the usage Anthropic just reported, clearing an earlier read's error.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the account was deleted.
pub async fn record_usage(
    connection: &mut AsyncPgConnection,
    llm_id: Uuid,
    usage: &UsageWindows,
    read_at: DateTime<Utc>,
) -> QueryResult<ClaudeAccount> {
    diesel::update(claude_accounts::table.find(llm_id))
        .set((
            claude_accounts::five_hour_utilization
                .eq(usage.five_hour.map(|window| window.utilization)),
            claude_accounts::five_hour_resets_at
                .eq(usage.five_hour.and_then(|window| window.resets_at)),
            claude_accounts::seven_day_utilization
                .eq(usage.seven_day.map(|window| window.utilization)),
            claude_accounts::seven_day_resets_at
                .eq(usage.seven_day.and_then(|window| window.resets_at)),
            claude_accounts::usage_read_at.eq(Some(read_at)),
            claude_accounts::usage_error.eq(None::<String>),
        ))
        .returning(ClaudeAccount::as_returning())
        .get_result(connection)
        .await
}

/// Records why usage could not be read, keeping the last usage that was.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] when the account was deleted.
pub async fn record_usage_error(
    connection: &mut AsyncPgConnection,
    llm_id: Uuid,
    reason: &str,
) -> QueryResult<ClaudeAccount> {
    diesel::update(claude_accounts::table.find(llm_id))
        .set(claude_accounts::usage_error.eq(Some(reason)))
        .returning(ClaudeAccount::as_returning())
        .get_result(connection)
        .await
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

    fn signed_in(account_uuid: Uuid, access_token: &str, refresh_token: &str) -> SignedIn {
        SignedIn {
            tokens: Tokens {
                access_token: SecretString::from(access_token),
                access_token_expires_at: Utc::now() + TimeDelta::hours(8),
                refresh_token: SecretString::from(refresh_token),
                refresh_token_expires_at: Some(Utc::now() + TimeDelta::days(30)),
            },
            identity: Identity {
                account_uuid,
                email: "alex@example.com".to_owned(),
                organization_uuid: None,
                organization_name: Some("Example".to_owned()),
            },
            subscription_type: Some("max".to_owned()),
        }
    }

    fn credential(name: &str) -> NewCredential<'_> {
        NewCredential {
            name,
            priority: 3,
            created_by: TEST_PERSON_ID,
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_sign_in_stores_a_credential_whose_token_is_the_access_token() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let account_uuid = Uuid::now_v7();

        let saved = save_sign_in(
            &mut connection,
            &cipher,
            credential("Work Claude"),
            &signed_in(account_uuid, "sk-ant-oat01-access", "sk-ant-ort01-refresh"),
        )
        .await
        .expect("saved");

        assert!(saved.is_new);
        assert_eq!(saved.llm.type_, LlmType::ClaudeAccount);
        assert_eq!(saved.llm.priority, 3);
        assert_eq!(saved.llm.expires_at, None);
        assert_eq!(
            saved
                .llm
                .secret_token(&cipher)
                .expect("opens")
                .expose_secret(),
            "sk-ant-oat01-access"
        );
        assert_eq!(
            saved
                .account
                .refresh_token(&cipher)
                .expect("opens")
                .expose_secret(),
            "sk-ant-ort01-refresh"
        );
        assert!(saved.account.is_signed_in());
        assert!(
            !saved
                .account
                .refresh_token_encrypted
                .windows(b"sk-ant-ort01-refresh".len())
                .any(|window| window == b"sk-ant-ort01-refresh"),
            "only ciphertext reaches Postgres"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn signing_an_account_in_again_replaces_its_tokens_and_clears_a_refused_sign_in() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let account_uuid = Uuid::now_v7();
        let first = save_sign_in(
            &mut connection,
            &cipher,
            credential("Work Claude"),
            &signed_in(account_uuid, "access-1", "refresh-1"),
        )
        .await
        .expect("saved");
        mark_expired(&mut connection, first.llm.id, "invalid_grant")
            .await
            .expect("marked");

        let again = save_sign_in(
            &mut connection,
            &cipher,
            credential("Another name"),
            &signed_in(account_uuid, "access-2", "refresh-2"),
        )
        .await
        .expect("saved again");

        assert!(!again.is_new);
        assert_eq!(again.llm.id, first.llm.id, "the same credential");
        assert_eq!(again.llm.name, "Work Claude", "its name and place are kept");
        assert!(again.account.is_signed_in());
        assert_eq!(again.account.last_refresh_error, None);
        assert_eq!(
            again
                .llm
                .secret_token(&cipher)
                .expect("opens")
                .expose_secret(),
            "access-2"
        );
        assert_eq!(
            again
                .account
                .refresh_token(&cipher)
                .expect("opens")
                .expose_secret(),
            "refresh-2"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn deleting_the_credential_deletes_its_account() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let saved = save_sign_in(
            &mut connection,
            &cipher,
            credential("Doomed"),
            &signed_in(Uuid::now_v7(), "access", "refresh"),
        )
        .await
        .expect("saved");

        llm::delete(&mut connection, saved.llm.id)
            .await
            .expect("deleted");
        assert!(
            find(&mut connection, saved.llm.id)
                .await
                .expect("query")
                .is_none()
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn usage_is_stored_and_read_back_as_windows() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let saved = save_sign_in(
            &mut connection,
            &cipher,
            credential("Usage"),
            &signed_in(Uuid::now_v7(), "access", "refresh"),
        )
        .await
        .expect("saved");
        let resets_at = Utc::now() + TimeDelta::hours(2);
        let usage = UsageWindows {
            five_hour: Some(UsageWindow {
                utilization: 42.5,
                resets_at: Some(resets_at),
            }),
            seven_day: None,
        };

        let stored = record_usage(&mut connection, saved.llm.id, &usage, Utc::now())
            .await
            .expect("recorded");
        let read = stored.usage();
        assert_eq!(read.five_hour.map(|window| window.utilization), Some(42.5));
        assert_eq!(
            read.five_hour
                .and_then(|window| window.resets_at)
                .map(|instant| instant.timestamp_micros()),
            Some(resets_at.timestamp_micros())
        );
        assert_eq!(read.seven_day, None);
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_copied_refresh_token_refuses_to_open() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let victim = save_sign_in(
            &mut connection,
            &cipher,
            credential("Victim"),
            &signed_in(Uuid::now_v7(), "access-v", "refresh-v"),
        )
        .await
        .expect("saved");
        let mut attacker = save_sign_in(
            &mut connection,
            &cipher,
            credential("Attacker"),
            &signed_in(Uuid::now_v7(), "access-a", "refresh-a"),
        )
        .await
        .expect("saved");

        attacker
            .account
            .refresh_token_encrypted
            .clone_from(&victim.account.refresh_token_encrypted);
        assert_eq!(
            attacker.account.refresh_token(&cipher).unwrap_err(),
            OpenError::Inauthentic
        );
    }
}
