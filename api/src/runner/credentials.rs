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

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use secrecy::SecretString;
use tracing::{Level, event};
use uuid::Uuid;

use super::command::CredentialSecret;
use crate::crypto::Cipher;
use crate::models::llm::{self, Llm, LlmType};

/// One credential a turn can run on.
#[derive(Debug, Clone)]
pub struct ClaudeCredential {
    pub id: Uuid,
    pub name: String,
    pub secret: CredentialSecret,
}

/// The usable Claude credentials, in the order a turn tries them.
///
/// # Errors
/// Propagates any database error.
pub async fn load(
    connection: &mut AsyncPgConnection,
    cipher: &Cipher,
    now: DateTime<Utc>,
) -> diesel::QueryResult<Vec<ClaudeCredential>> {
    let stored = llm::list(connection).await?;
    let mut usable = Vec::with_capacity(stored.len());
    for credential in stored {
        if !is_usable(&credential, now) {
            continue;
        }
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
                name: credential.name,
                secret,
            });
        }
    }
    Ok(usable)
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
        // `claude setup-token` mints a subscription token, presented as a bearer.
        LlmType::ClaudeCodeOauth => Some(CredentialSecret::SubscriptionToken(token)),
        LlmType::ClaudeApiToken => Some(CredentialSecret::ApiKey(token)),
        LlmType::ChatgptApiToken | LlmType::ChatgptOauth => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::*;
    use crate::test_support::TEST_PERSON_ID;

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
        assert!(secret_for(LlmType::ChatgptOauth, token()).is_none());
        assert!(secret_for(LlmType::ChatgptApiToken, token()).is_none());
    }
}
