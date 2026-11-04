// Copyright © 2026 Jalapeno Labs

//! The Claude credentials a turn fails over through, highest priority first.
//!
//! The stored LLM credentials are already ordered by priority, then age. A turn runs on the
//! first usable one; when that one is spent or refused, the next takes over (see
//! `super::turn`). The list is failover, never a pool: a later credential serves only once the
//! ones above it cannot.
//!
//! Only Claude credentials qualify, because the runner drives the `claude` CLI. A `ChatGPT`
//! credential is skipped, as is an inactive or expired one, and one that cannot be decrypted
//! is skipped and logged.
//!
//! A Claude account signed in through OAuth is handed its current access token, never its
//! refresh token (see `crate::claude_accounts`). It is skipped while its sign-in is expired or
//! its access token has lapsed, and while Anthropic's latest usage shows a window spent: a turn
//! started on it would give out at once. When nothing else is usable, the turn is usage-limited
//! until the earliest of those windows resets.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;
use tracing::{Level, event};
use uuid::Uuid;

use super::command::CredentialSecret;
use crate::claude_accounts::usage;
use crate::crypto::Cipher;
use crate::models::claude_account::{self, ClaudeAccount};
use crate::models::llm::{self, Llm, LlmType};

/// One credential a turn can run on.
#[derive(Debug, Clone)]
pub struct ClaudeCredential {
    pub id: Uuid,
    pub name: String,
    pub secret: CredentialSecret,
}

/// The credentials a turn can run on, and when the ones held back by usage come back.
#[derive(Debug, Clone, Default)]
pub struct LoadedCredentials {
    /// In the order a turn tries them.
    pub usable: Vec<ClaudeCredential>,
    /// The earliest reset among Claude accounts skipped because a usage window is spent;
    /// `None` when none was.
    pub limited_until: Option<DateTime<Utc>>,
}

/// The usable Claude credentials, in the order a turn tries them.
///
/// # Errors
/// Propagates any database error.
pub async fn load(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    now: DateTime<Utc>,
) -> diesel::QueryResult<LoadedCredentials> {
    let stored = llm::list(connection).await?;
    let accounts = accounts_by_credential(connection).await?;
    let sorted = sort_out(&stored, &accounts, now);
    for (credential, hold) in &sorted.held {
        match hold {
            AccountHold::Ready => {}
            AccountHold::Unavailable(reason) => event!(
                name: "runner.credential.account_unavailable",
                Level::DEBUG,
                llm.id = %credential.id,
                reason,
                "a Claude account was skipped",
            ),
            AccountHold::UsageLimited(until) => event!(
                name: "runner.credential.usage_limited",
                Level::INFO,
                llm.id = %credential.id,
                usage.resets_at = %until,
                "a Claude account's usage is spent; it was skipped",
            ),
        }
    }
    let mut usable = Vec::with_capacity(sorted.serving.len());
    for credential in sorted.serving {
        let token = match credential.secret_token(cipher) {
            Ok(token) => token,
            Err(error) => {
                event!(
                    name: "runner.credential.unreadable",
                    Level::ERROR,
                    llm.id = %credential.id,
                    error.message = %error,
                    "a stored credential could not be decrypted and was skipped",
                );
                continue;
            }
        };
        if let Some(secret) = secret_for(credential.type_, token) {
            usable.push(ClaudeCredential {
                id: credential.id,
                name: credential.name.clone(),
                secret,
            });
        }
    }
    Ok(LoadedCredentials {
        usable,
        limited_until: sorted.limited_until,
    })
}

/// Until when every Claude credential that could serve a turn is held back by its usage: the
/// earliest reset among them. `None` while any credential could serve, and while none is stored
/// at all, when a turn runs on the workspace's own login. The rule [`load`] follows, read without
/// decrypting anything, for what only needs to know whether a turn would run, such as the worker
/// pool's scheduler (`crate::worker_pool`).
///
/// # Errors
/// Propagates any database error.
pub async fn usage_hold(
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> diesel::QueryResult<Option<DateTime<Utc>>> {
    let stored = llm::list(connection).await?;
    let accounts = accounts_by_credential(connection).await?;
    let sorted = sort_out(&stored, &accounts, now);
    if sorted.serving.is_empty() {
        return Ok(sorted.limited_until);
    }
    Ok(None)
}

async fn accounts_by_credential(
    connection: &mut AsyncPgConnection,
) -> diesel::QueryResult<HashMap<Uuid, ClaudeAccount>> {
    let accounts = claude_account::list(connection).await?;
    Ok(accounts
        .into_iter()
        .map(|account| (account.llm_id, account))
        .collect())
}

/// The stored credentials sorted out at a moment, before any is decrypted.
#[derive(Debug)]
struct Sorted<'a> {
    /// Those a turn may run on, in the order it tries them.
    serving: Vec<&'a Llm>,
    /// The Claude accounts left out, and why.
    held: Vec<(&'a Llm, AccountHold)>,
    /// The earliest reset among the accounts left out because a usage window is spent.
    limited_until: Option<DateTime<Utc>>,
}

/// Sorts `stored` credentials out at `now`: an inactive or expired one, or one the CLI cannot
/// drive, is left out; a Claude account that cannot serve is held back, with the earliest reset
/// among those held by usage.
fn sort_out<'a>(
    stored: &'a [Llm],
    accounts: &HashMap<Uuid, ClaudeAccount>,
    now: DateTime<Utc>,
) -> Sorted<'a> {
    let mut sorted = Sorted {
        serving: Vec::with_capacity(stored.len()),
        held: Vec::new(),
        limited_until: None,
    };
    for credential in stored {
        if !is_usable(credential, now) || !drives_the_cli(credential.type_) {
            continue;
        }
        if credential.type_ == LlmType::ClaudeAccount {
            let hold = account_hold(accounts.get(&credential.id), now);
            if let AccountHold::UsageLimited(until) = hold {
                sorted.limited_until = Some(
                    sorted
                        .limited_until
                        .map_or(until, |earliest| earliest.min(until)),
                );
            }
            if hold != AccountHold::Ready {
                sorted.held.push((credential, hold));
                continue;
            }
        }
        sorted.serving.push(credential);
    }
    sorted
}

/// Whether the `claude` CLI can run on a credential of `type_`.
const fn drives_the_cli(type_: LlmType) -> bool {
    match type_ {
        LlmType::ClaudeCodeOauth | LlmType::ClaudeAccount | LlmType::ClaudeApiToken => true,
        LlmType::ChatgptApiToken | LlmType::ChatgptOauth => false,
    }
}

/// Whether a Claude account can take a turn now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AccountHold {
    Ready,
    /// It cannot serve until someone acts: no account row, an expired sign-in, or a lapsed
    /// access token the keeper has not renewed.
    Unavailable(&'static str),
    /// A usage window is spent until this instant.
    UsageLimited(DateTime<Utc>),
}

fn account_hold(account: Option<&ClaudeAccount>, now: DateTime<Utc>) -> AccountHold {
    let Some(account) = account else {
        return AccountHold::Unavailable("the credential has no signed-in account");
    };
    if !account.is_signed_in() {
        return AccountHold::Unavailable("the account's sign-in expired");
    }
    if account.access_token_expires_at <= now {
        return AccountHold::Unavailable("the account's access token lapsed");
    }
    usage::blocked_until(&account.usage(), now)
        .map_or(AccountHold::Ready, AccountHold::UsageLimited)
}

fn is_usable(credential: &Llm, now: DateTime<Utc>) -> bool {
    credential.is_active
        && credential
            .expires_at
            .is_none_or(|expires_at| expires_at > now)
}

/// How the CLI is handed a credential of `type_`; `None` for one it cannot use.
fn secret_for(type_: LlmType, token: SecretString) -> Option<CredentialSecret> {
    match type_ {
        // `claude setup-token` mints a subscription token, presented as a bearer; a signed-in
        // account's access token is presented the same way.
        LlmType::ClaudeCodeOauth | LlmType::ClaudeAccount => {
            Some(CredentialSecret::SubscriptionToken(token))
        }
        LlmType::ClaudeApiToken => Some(CredentialSecret::ApiKey(token)),
        LlmType::ChatgptApiToken | LlmType::ChatgptOauth => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use secrecy::ExposeSecret;

    use super::*;
    use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database};

    fn stored(type_: LlmType, is_active: bool, expires_at: Option<DateTime<Utc>>) -> Llm {
        Llm {
            created_by: TEST_PERSON_ID,
            id: Uuid::now_v7(),
            name: "Credential".to_owned(),
            description: String::new(),
            type_,
            secret_token_encrypted: Vec::new(),
            priority: 0,
            is_active,
            expires_at,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn inactive_and_expired_credentials_are_skipped() {
        let now = Utc::now();
        assert!(is_usable(
            &stored(LlmType::ClaudeCodeOauth, true, None),
            now
        ));
        assert!(!is_usable(
            &stored(LlmType::ClaudeCodeOauth, false, None),
            now
        ));
        assert!(!is_usable(
            &stored(
                LlmType::ClaudeCodeOauth,
                true,
                Some(now - TimeDelta::minutes(1))
            ),
            now
        ));
        assert!(is_usable(
            &stored(
                LlmType::ClaudeApiToken,
                true,
                Some(now + TimeDelta::days(1))
            ),
            now
        ));
    }

    fn account(
        sign_in_state: &str,
        access_left: TimeDelta,
        five_hour: Option<f64>,
    ) -> ClaudeAccount {
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
            refresh_token_expires_at: None,
            sign_in_state: sign_in_state.to_owned(),
            last_refreshed_at: None,
            last_refresh_error: None,
            five_hour_utilization: five_hour,
            five_hour_resets_at: five_hour.map(|_utilization| now + TimeDelta::hours(2)),
            seven_day_utilization: None,
            seven_day_resets_at: None,
            usage_read_at: None,
            usage_error: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn a_signed_in_account_with_room_left_is_ready() {
        let now = Utc::now();
        let busy = account(claude_account::SIGNED_IN, TimeDelta::hours(5), Some(40.0));
        assert_eq!(account_hold(Some(&busy), now), AccountHold::Ready);
        let unread = account(claude_account::SIGNED_IN, TimeDelta::hours(5), None);
        assert_eq!(
            account_hold(Some(&unread), now),
            AccountHold::Ready,
            "usage not read yet does not hold an account back"
        );
    }

    #[test]
    fn an_account_whose_window_is_spent_is_held_until_it_resets() {
        let now = Utc::now();
        let spent = account(claude_account::SIGNED_IN, TimeDelta::hours(5), Some(100.0));
        let resets_at = spent.five_hour_resets_at.expect("set");
        assert_eq!(
            account_hold(Some(&spent), now),
            AccountHold::UsageLimited(resets_at)
        );
    }

    #[test]
    fn an_expired_or_lapsed_account_is_unavailable() {
        let now = Utc::now();
        let expired = account(claude_account::EXPIRED, TimeDelta::hours(5), None);
        assert!(matches!(
            account_hold(Some(&expired), now),
            AccountHold::Unavailable(_)
        ));
        let lapsed = account(claude_account::SIGNED_IN, -TimeDelta::minutes(1), None);
        assert!(matches!(
            account_hold(Some(&lapsed), now),
            AccountHold::Unavailable(_)
        ));
        assert!(matches!(
            account_hold(None, now),
            AccountHold::Unavailable(_)
        ));
    }

    #[test]
    fn only_claude_credentials_drive_the_cli() {
        let token = || SecretString::from("token");
        assert!(matches!(
            secret_for(LlmType::ClaudeCodeOauth, token()),
            Some(CredentialSecret::SubscriptionToken(_))
        ));
        assert!(matches!(
            secret_for(LlmType::ClaudeApiToken, token()),
            Some(CredentialSecret::ApiKey(_))
        ));
        assert!(matches!(
            secret_for(LlmType::ClaudeAccount, token()),
            Some(CredentialSecret::SubscriptionToken(_))
        ));
        assert!(secret_for(LlmType::ChatgptOauth, token()).is_none());
        assert!(secret_for(LlmType::ChatgptApiToken, token()).is_none());
    }

    async fn signed_in_account(
        connection: &mut AsyncPgConnection,
        cipher: &Cipher,
        name: &str,
        priority: i32,
        five_hour: f64,
    ) -> Uuid {
        use crate::models::claude_account::{
            Identity, NewCredential, SignedIn, Tokens, UsageWindow, UsageWindows,
        };

        let saved = claude_account::save_sign_in(
            connection,
            cipher,
            NewCredential {
                name,
                priority,
                created_by: TEST_PERSON_ID,
            },
            &SignedIn {
                tokens: Tokens {
                    access_token: SecretString::from(format!("access-{name}")),
                    access_token_expires_at: Utc::now() + TimeDelta::hours(6),
                    refresh_token: SecretString::from(format!("refresh-{name}")),
                    refresh_token_expires_at: None,
                },
                identity: Identity {
                    account_uuid: Uuid::now_v7(),
                    email: format!("{name}@example.com"),
                    organization_uuid: None,
                    organization_name: None,
                },
                subscription_type: None,
            },
        )
        .await
        .expect("saved");
        claude_account::record_usage(
            connection,
            saved.llm.id,
            &UsageWindows {
                five_hour: Some(UsageWindow {
                    utilization: five_hour,
                    resets_at: Some(Utc::now() + TimeDelta::hours(2)),
                }),
                seven_day: None,
            },
            Utc::now(),
        )
        .await
        .expect("usage stored");
        saved.llm.id
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_spent_account_is_skipped_and_the_next_credential_serves() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        let spent = signed_in_account(&mut connection, &cipher, "spent", 0, 100.0).await;
        let fresh = signed_in_account(&mut connection, &cipher, "fresh", 1, 20.0).await;

        let loaded = load(&mut connection, &cipher, Utc::now())
            .await
            .expect("loaded");

        let ids: Vec<Uuid> = loaded
            .usable
            .iter()
            .map(|credential| credential.id)
            .collect();
        assert_eq!(
            ids,
            vec![fresh],
            "the spent account is left out, the next serves"
        );
        assert!(!ids.contains(&spent));
        assert!(loaded.limited_until.is_some());
        assert!(matches!(
            &loaded.usable[0].secret,
            CredentialSecret::SubscriptionToken(token) if token.expose_secret() == "access-fresh"
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn when_every_account_is_spent_the_earliest_reset_is_reported() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        signed_in_account(&mut connection, &cipher, "first", 0, 100.0).await;
        signed_in_account(&mut connection, &cipher, "second", 1, 99.5).await;

        let loaded = load(&mut connection, &cipher, Utc::now())
            .await
            .expect("loaded");

        assert!(loaded.usable.is_empty());
        let resets_at = loaded.limited_until.expect("a reset to wait for");
        assert!(resets_at > Utc::now() && resets_at <= Utc::now() + TimeDelta::hours(2));
    }

    #[test]
    fn usage_holds_turns_only_when_every_credential_that_could_serve_is_spent() {
        let now = Utc::now();
        let spent = stored(LlmType::ClaudeAccount, true, None);
        let mut spent_account =
            account(claude_account::SIGNED_IN, TimeDelta::hours(5), Some(100.0));
        spent_account.llm_id = spent.id;
        let resets_at = spent_account.five_hour_resets_at.expect("set");
        let accounts = HashMap::from([(spent.id, spent_account)]);

        let alone = [spent.clone()];
        let sorted = sort_out(&alone, &accounts, now);
        assert!(sorted.serving.is_empty());
        assert_eq!(sorted.limited_until, Some(resets_at));

        let key = stored(LlmType::ClaudeApiToken, true, None);
        let with_a_key = [spent.clone(), key.clone()];
        let sorted = sort_out(&with_a_key, &accounts, now);
        let serving: Vec<Uuid> = sorted
            .serving
            .iter()
            .map(|credential| credential.id)
            .collect();
        assert_eq!(
            serving,
            vec![key.id],
            "an API key still serves while the account rests"
        );

        let chatgpt = stored(LlmType::ChatgptApiToken, true, None);
        let with_chatgpt = [spent, chatgpt];
        assert!(
            sort_out(&with_chatgpt, &accounts, now).serving.is_empty(),
            "a ChatGPT credential never runs the CLI, so it does not lift the hold"
        );
        assert!(sort_out(&[], &accounts, now).limited_until.is_none());
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn the_usage_hold_follows_the_stored_accounts() {
        let (_url, mut connection) = migrated_database().await;
        let cipher = cipher();
        assert_eq!(
            usage_hold(&mut connection, Utc::now()).await.expect("read"),
            None,
            "with no credential at all, a turn runs on the workspace's login"
        );

        signed_in_account(&mut connection, &cipher, "first", 0, 100.0).await;
        let held = usage_hold(&mut connection, Utc::now())
            .await
            .expect("read")
            .expect("every account is spent");
        assert!(held > Utc::now());

        signed_in_account(&mut connection, &cipher, "second", 1, 30.0).await;
        assert_eq!(
            usage_hold(&mut connection, Utc::now()).await.expect("read"),
            None
        );
    }
}
