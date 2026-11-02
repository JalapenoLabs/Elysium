// Copyright © 2026 Jalapeno Labs

//! The keeper: refreshes every signed-in account before its token lapses, and reads its usage.
//!
//! Elysium is the only process that refreshes an account. Turns are handed the current access
//! token alone, never the refresh token, because Claude Code rotates the refresh token on every
//! refresh: two refreshers would strand each other. Within Elysium, a refresh holds the
//! account's row lock (`claude_account::lock`) from reading the old refresh token until the new
//! one is stored, so two passes, or two API processes, never send the same token.
//!
//! An access token lives about eight hours. The keeper renews it once fewer than
//! [`ACCESS_TOKEN_RENEW_BEFORE`] remain, so a turn always starts with hours of token left, and
//! renews regardless once the refresh token is within [`REFRESH_TOKEN_RENEW_BEFORE`] of expiring,
//! so an account nobody uses never dies of disuse. Refreshing never cuts a running turn short: the
//! access token it was handed stays valid until its own expiry.
//!
//! When Anthropic refuses a refresh token (`400` or `401`), the account is marked "Sign-in
//! expired" once and announced; the keeper does not try it again until a new sign-in replaces
//! the token. Any other failure is recorded and retried on the next pass.
//!
//! The one window that can lose an account: Anthropic rotates the token, and the transaction
//! storing the new one then fails to commit. The old token is gone and the new one with it; the
//! next pass is refused and marks the account expired, and signing in again recovers it.

use chrono::{DateTime, TimeDelta, Utc};
use diesel_async::AsyncConnection;
use tracing::{Level, event};
use uuid::Uuid;

use super::Inner;
use crate::models::claude_account::{self, ClaudeAccount};
use crate::models::llm::{self, Llm};
use crate::realtime::ServerEvent;
use crate::routes::v1::llms::{LlmResponse, UsageView};

/// How often every account is looked at. Usage is read on the same pass, and a finished turn asks
/// for a pass at once.
pub const PASS_INTERVAL: std::time::Duration = std::time::Duration::from_mins(5);

/// Renew an access token once fewer than this many hours of it remain. Access tokens last about
/// eight hours, so this refreshes roughly every five, and every turn starts with at least this
/// long before its token lapses; a turn running longer than that rolls to the next credential.
pub const ACCESS_TOKEN_RENEW_BEFORE: TimeDelta = TimeDelta::hours(3);

/// Renew regardless once the refresh token is this close to expiring, so an idle account stays
/// signed in. Each refresh rolls the refresh token forward.
pub const REFRESH_TOKEN_RENEW_BEFORE: TimeDelta = TimeDelta::days(7);

/// Whether an account's tokens should be renewed now.
pub fn needs_refresh(account: &ClaudeAccount, now: DateTime<Utc>) -> bool {
    if !account.is_signed_in() {
        return false;
    }
    let access_is_low = account.access_token_expires_at - now < ACCESS_TOKEN_RENEW_BEFORE;
    let refresh_is_low = account
        .refresh_token_expires_at
        .is_some_and(|expires_at| expires_at - now < REFRESH_TOKEN_RENEW_BEFORE);
    access_is_low || refresh_is_low
}

/// What one refresh attempt came to.
#[derive(Debug)]
enum Refresh {
    /// Another pass renewed it first, or it no longer needs renewing.
    NotNeeded,
    Refreshed(Llm, ClaudeAccount),
    /// Anthropic refused the refresh token; the account is now expired.
    Rejected(ClaudeAccount),
    /// Anthropic could not be reached or failed; retried next pass.
    Failed,
    /// The sealed refresh token would not open, so it can never be sent.
    Unreadable(ClaudeAccount),
}

/// One pass over every account.
pub(super) async fn run_pass(inner: &Inner) {
    let now = Utc::now();
    let accounts = match inner.database.get().await {
        Ok(mut connection) => claude_account::list(&mut connection).await,
        Err(error) => {
            event!(
                name: "claude_accounts.keeper.no_database",
                Level::WARN,
                error.message = %error,
                "the keeper could not reach the database; it tries again next pass",
            );
            return;
        }
    };
    let accounts = match accounts {
        Ok(accounts) => accounts,
        Err(error) => {
            event!(
                name: "claude_accounts.keeper.list_failed",
                Level::WARN,
                error.message = %error,
                "the keeper could not list accounts; it tries again next pass",
            );
            return;
        }
    };

    for account in accounts {
        if !account.is_signed_in() {
            continue;
        }
        if needs_refresh(&account, now) {
            refresh(inner, account.llm_id, now).await;
        }
        read_usage(inner, account.llm_id).await;
    }
}

async fn refresh(inner: &Inner, llm_id: Uuid, now: DateTime<Utc>) {
    let mut connection = match inner.database.get().await {
        Ok(connection) => connection,
        Err(error) => {
            event!(
                name: "claude_accounts.refresh.no_database",
                Level::WARN,
                llm.id = %llm_id,
                error.message = %error,
                "an account could not be refreshed; it is tried again next pass",
            );
            return;
        }
    };
    let outcome = trade_under_lock(inner, &mut connection, llm_id, now).await;

    match outcome {
        Ok(Refresh::NotNeeded) => {}
        Ok(Refresh::Refreshed(llm, account)) => {
            event!(
                name: "claude_accounts.refresh.succeeded",
                Level::INFO,
                llm.id = %llm_id,
                token.expires_at = %account.access_token_expires_at,
                "a Claude account's tokens were renewed",
            );
            inner
                .events
                .publish(&ServerEvent::LlmUpserted(LlmResponse::new(
                    llm,
                    Some(&account),
                )));
        }
        Ok(Refresh::Rejected(account) | Refresh::Unreadable(account)) => {
            event!(
                name: "claude_accounts.refresh.rejected",
                Level::WARN,
                llm.id = %llm_id,
                error.message = account.last_refresh_error.as_deref().unwrap_or_default(),
                "Anthropic refused a Claude account's sign-in; it must be signed in again",
            );
            publish_account(inner, &mut connection, account).await;
        }
        Ok(Refresh::Failed) => {
            event!(
                name: "claude_accounts.refresh.failed",
                Level::WARN,
                llm.id = %llm_id,
                "a Claude account could not be refreshed; it is tried again next pass",
            );
        }
        Err(diesel::result::Error::NotFound) => {
            event!(
                name: "claude_accounts.refresh.deleted",
                Level::DEBUG,
                llm.id = %llm_id,
                "an account was deleted while the keeper looked at it",
            );
        }
        Err(error) => {
            event!(
                name: "claude_accounts.refresh.database_failed",
                Level::ERROR,
                llm.id = %llm_id,
                error.message = %error,
                "a Claude account's refresh could not be stored",
            );
        }
    }
}

/// Trades an account's refresh token inside one transaction that holds the account's row lock
/// from reading the old token until the new pair (or the refusal) is stored. A second pass waits
/// on the lock, then finds the account renewed and does nothing.
async fn trade_under_lock(
    inner: &Inner,
    connection: &mut diesel_async::AsyncPgConnection,
    llm_id: Uuid,
    now: DateTime<Utc>,
) -> diesel::QueryResult<Refresh> {
    let anthropic = &inner.anthropic;
    let cipher = &*inner.cipher;
    connection
        .transaction(async move |connection| {
            let account = claude_account::lock(connection, llm_id).await?;
            if !needs_refresh(&account, now) {
                return Ok(Refresh::NotNeeded);
            }
            let Ok(refresh_token) = account.refresh_token(cipher) else {
                let account = claude_account::mark_expired(
                    connection,
                    llm_id,
                    "the stored sign-in could not be decrypted; sign in again",
                )
                .await?;
                return Ok(Refresh::Unreadable(account));
            };
            match anthropic
                .refresh(&refresh_token, now, account.refresh_token_expires_at)
                .await
            {
                Ok(tokens) => {
                    let (llm, account) =
                        claude_account::store_refreshed(connection, cipher, llm_id, &tokens, now)
                            .await?;
                    Ok(Refresh::Refreshed(llm, account))
                }
                Err(error) if error.is_token_rejected() => {
                    let account =
                        claude_account::mark_expired(connection, llm_id, &error.to_string())
                            .await?;
                    Ok(Refresh::Rejected(account))
                }
                Err(error) => {
                    claude_account::record_refresh_error(connection, llm_id, &error.to_string())
                        .await?;
                    Ok(Refresh::Failed)
                }
            }
        })
        .await
}

/// Reads one account's usage with its current access token, and announces a change.
async fn read_usage(inner: &Inner, llm_id: Uuid) {
    let Ok(mut connection) = inner.database.get().await else {
        event!(
            name: "claude_accounts.usage.no_database",
            Level::DEBUG,
            llm.id = %llm_id,
            "usage was not read; the database is unreachable",
        );
        return;
    };
    let (Ok(credential), Ok(Some(before))) = (
        llm::find(&mut connection, llm_id).await,
        claude_account::find(&mut connection, llm_id).await,
    ) else {
        event!(
            name: "claude_accounts.usage.missing",
            Level::DEBUG,
            llm.id = %llm_id,
            "the account is gone; its usage is not read",
        );
        return;
    };
    if !before.is_signed_in() || before.access_token_expires_at <= Utc::now() {
        event!(
            name: "claude_accounts.usage.no_token",
            Level::DEBUG,
            llm.id = %llm_id,
            "the account has no live access token; its usage is not read",
        );
        return;
    }
    let Ok(access_token) = credential.secret_token(&inner.cipher) else {
        event!(
            name: "claude_accounts.usage.unreadable",
            Level::ERROR,
            llm.id = %llm_id,
            "an account's access token could not be decrypted",
        );
        return;
    };

    let stored = match inner.anthropic.usage(&access_token).await {
        Ok(usage) => {
            claude_account::record_usage(&mut connection, llm_id, &usage, Utc::now()).await
        }
        Err(error) => {
            event!(
                name: "claude_accounts.usage.failed",
                Level::INFO,
                llm.id = %llm_id,
                error.message = %error,
                "an account's usage could not be read",
            );
            claude_account::record_usage_error(&mut connection, llm_id, &error.to_string()).await
        }
    };
    match stored {
        Ok(after) => {
            if usage_changed(&before, &after) {
                inner.events.publish(&ServerEvent::LlmUsage {
                    id: llm_id,
                    usage: UsageView::from(&after),
                });
            }
        }
        Err(error) => {
            event!(
                name: "claude_accounts.usage.store_failed",
                Level::WARN,
                llm.id = %llm_id,
                error.message = %error,
                "an account's usage could not be stored",
            );
        }
    }
}

/// Whether what a person sees of an account's usage changed: a window's figure or reset, or a
/// read starting or stopping to fail.
fn usage_changed(before: &ClaudeAccount, after: &ClaudeAccount) -> bool {
    before.usage() != after.usage() || before.usage_error.is_some() != after.usage_error.is_some()
}

async fn publish_account(
    inner: &Inner,
    connection: &mut diesel_async::AsyncPgConnection,
    account: ClaudeAccount,
) {
    match llm::find(connection, account.llm_id).await {
        Ok(credential) => inner
            .events
            .publish(&ServerEvent::LlmUpserted(LlmResponse::new(
                credential,
                Some(&account),
            ))),
        Err(error) => event!(
            name: "claude_accounts.publish.missing",
            Level::DEBUG,
            llm.id = %account.llm_id,
            error.message = %error,
            "an account's credential is gone; nothing to announce",
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use secrecy::{ExposeSecret, SecretString};

    use super::super::fake::FakeAnthropic;
    use super::super::tests::accounts_against;
    use super::*;
    use crate::crypto::Cipher;
    use crate::models::claude_account::{
        EXPIRED, Identity, NewCredential, SIGNED_IN, SignedIn, Tokens,
    };
    use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

    fn account(access_left: TimeDelta, refresh_left: Option<TimeDelta>) -> ClaudeAccount {
        let now = Utc::now();
        ClaudeAccount {
            llm_id: Uuid::now_v7(),
            account_uuid: Uuid::now_v7(),
            email: "alex@example.com".to_owned(),
            organization_uuid: None,
            organization_name: None,
            subscription_type: None,
            refresh_token_encrypted: Vec::new(),
            access_token_expires_at: now + access_left,
            refresh_token_expires_at: refresh_left.map(|left| now + left),
            sign_in_state: SIGNED_IN.to_owned(),
            last_refreshed_at: None,
            last_refresh_error: None,
            five_hour_utilization: None,
            five_hour_resets_at: None,
            seven_day_utilization: None,
            seven_day_resets_at: None,
            usage_read_at: None,
            usage_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn an_access_token_is_renewed_once_fewer_than_three_hours_remain() {
        let now = Utc::now();
        assert!(!needs_refresh(&account(TimeDelta::hours(6), None), now));
        assert!(needs_refresh(&account(TimeDelta::hours(2), None), now));
        assert!(
            needs_refresh(&account(-TimeDelta::minutes(5), None), now),
            "an expired token too"
        );
    }

    #[test]
    fn an_idle_account_is_renewed_before_its_refresh_token_expires() {
        let now = Utc::now();
        assert!(!needs_refresh(
            &account(TimeDelta::hours(7), Some(TimeDelta::days(20))),
            now
        ));
        assert!(needs_refresh(
            &account(TimeDelta::hours(7), Some(TimeDelta::days(6))),
            now
        ));
    }

    #[test]
    fn an_expired_sign_in_is_never_refreshed() {
        let mut expired = account(-TimeDelta::hours(1), None);
        expired.sign_in_state = EXPIRED.to_owned();
        assert!(!needs_refresh(&expired, Utc::now()));
    }

    /// An account sealed under `cipher`, which the service under test must share: the fixture's
    /// `cipher()` draws a fresh key on every call.
    async fn stored_account(
        connection: &mut diesel_async::AsyncPgConnection,
        cipher: &Cipher,
        access_left: TimeDelta,
    ) -> Uuid {
        let saved = claude_account::save_sign_in(
            connection,
            cipher,
            NewCredential {
                name: "Keeper test",
                priority: 0,
                created_by: TEST_PERSON_ID,
            },
            &SignedIn {
                tokens: Tokens {
                    access_token: SecretString::from(FakeAnthropic::ACCESS_TOKEN),
                    access_token_expires_at: Utc::now() + access_left,
                    refresh_token: SecretString::from(FakeAnthropic::REFRESH_TOKEN),
                    refresh_token_expires_at: None,
                },
                identity: Identity {
                    account_uuid: FakeAnthropic::ACCOUNT_UUID,
                    email: "alex@example.com".to_owned(),
                    organization_uuid: None,
                    organization_name: None,
                },
                subscription_type: Some("max".to_owned()),
            },
        )
        .await
        .expect("saved");
        saved.llm.id
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_refresh_stores_the_rotated_pair_and_hands_turns_the_new_access_token() {
        let (url, mut connection) = migrated_database().await;
        let fake = FakeAnthropic::start().await;
        let cipher = Arc::new(cipher());
        let llm_id = stored_account(&mut connection, &cipher, TimeDelta::hours(1)).await;
        let accounts = accounts_against(&url, &fake, &cipher).await;

        run_pass(&accounts.inner).await;

        assert_eq!(fake.refreshes(), 1);
        let credential = llm::find(&mut connection, llm_id)
            .await
            .expect("credential");
        assert_eq!(
            credential
                .secret_token(&cipher)
                .expect("opens")
                .expose_secret(),
            "access-rotated-1"
        );
        let stored = claude_account::find(&mut connection, llm_id)
            .await
            .expect("query")
            .expect("account");
        assert_eq!(
            stored
                .refresh_token(&cipher)
                .expect("opens")
                .expose_secret(),
            fake.current_refresh_token(),
            "the rotated refresh token is the one stored"
        );
        assert!(stored.access_token_expires_at > Utc::now() + TimeDelta::hours(7));
        assert!(
            stored.usage_read_at.is_some(),
            "usage is read on the same pass"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn concurrent_passes_send_each_refresh_token_once() {
        let (url, mut connection) = migrated_database().await;
        let fake = FakeAnthropic::start().await;
        let cipher = Arc::new(cipher());
        let llm_id = stored_account(&mut connection, &cipher, TimeDelta::hours(1)).await;
        let first = accounts_against(&url, &fake, &cipher).await;
        let second = accounts_against(&url, &fake, &cipher).await;

        tokio::join!(run_pass(&first.inner), run_pass(&second.inner));

        assert_eq!(
            fake.refreshes(),
            1,
            "the second pass waits on the row lock, then finds the account already renewed"
        );
        let stored = claude_account::find(&mut connection, llm_id)
            .await
            .expect("query")
            .expect("account");
        assert!(stored.is_signed_in(), "nobody sent a retired token");
        assert_eq!(
            stored
                .refresh_token(&cipher)
                .expect("opens")
                .expose_secret(),
            fake.current_refresh_token()
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_refused_refresh_token_marks_the_account_expired_and_is_not_retried() {
        let (url, mut connection) = migrated_database().await;
        let fake = FakeAnthropic::start().await;
        let cipher = Arc::new(cipher());
        let llm_id = stored_account(&mut connection, &cipher, TimeDelta::hours(1)).await;
        let accounts = accounts_against(&url, &fake, &cipher).await;
        // Someone else used the token first, so the fake has rotated past it.
        fake.client()
            .refresh(
                &SecretString::from(FakeAnthropic::REFRESH_TOKEN),
                Utc::now(),
                None,
            )
            .await
            .expect("rotated elsewhere");

        run_pass(&accounts.inner).await;
        run_pass(&accounts.inner).await;

        let stored = claude_account::find(&mut connection, llm_id)
            .await
            .expect("query")
            .expect("account");
        assert!(!stored.is_signed_in());
        assert!(
            stored
                .last_refresh_error
                .as_deref()
                .is_some_and(|reason| reason.contains("Refresh token not found")),
        );
        let token_requests = fake.token_requests();
        assert_eq!(
            token_requests, 2,
            "one from elsewhere, one refused; no retry after that"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn usage_is_stored_and_announced_only_when_it_changes() {
        let (url, mut connection) = migrated_database().await;
        let fake = FakeAnthropic::start().await;
        let cipher = Arc::new(cipher());
        // Six hours of token left: no refresh, so the pass only reads usage.
        let llm_id = stored_account(&mut connection, &cipher, TimeDelta::hours(6)).await;
        let accounts = accounts_against(&url, &fake, &cipher).await;
        let mut events = accounts.inner.events.subscribe();
        fake.set_usage(serde_json::json!({
            "five_hour": { "utilization": 100.0, "resets_at": "2099-01-01T05:00:00Z" },
        }));

        run_pass(&accounts.inner).await;
        let announced = events.try_recv().expect("the new usage is announced");
        assert!(announced.contains("\"type\":\"llm.usage\""));
        assert!(
            !announced.contains("alex@example.com"),
            "the usage event names no one"
        );
        let stored = claude_account::find(&mut connection, llm_id)
            .await
            .expect("query")
            .expect("account");
        assert_eq!(stored.five_hour_utilization, Some(100.0));
        assert_eq!(fake.refreshes(), 0);

        run_pass(&accounts.inner).await;
        assert!(
            events.try_recv().is_err(),
            "the same usage again is not announced"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_outage_is_recorded_and_retried_without_expiring_the_account() {
        let (url, mut connection) = migrated_database().await;
        let fake = FakeAnthropic::start().await;
        let cipher = Arc::new(cipher());
        let llm_id = stored_account(&mut connection, &cipher, TimeDelta::hours(1)).await;
        let accounts = accounts_against(&url, &fake, &cipher).await;

        fake.set_down(true);
        run_pass(&accounts.inner).await;
        let after_outage = claude_account::find(&mut connection, llm_id)
            .await
            .expect("query")
            .expect("account");
        assert!(after_outage.is_signed_in());
        assert!(after_outage.last_refresh_error.is_some());

        fake.set_down(false);
        run_pass(&accounts.inner).await;
        let recovered = claude_account::find(&mut connection, llm_id)
            .await
            .expect("query")
            .expect("account");
        assert_eq!(fake.refreshes(), 1);
        assert_eq!(recovered.last_refresh_error, None);
    }
}
