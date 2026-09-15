// Copyright © 2026 Jalapeno Labs

//! Elysium's stored credentials, shaped into a thread's model endpoints.
//!
//! The satellite walks a thread's `models` list strictly in order: the first entry is
//! tried until its retry policy is spent, then the next. So the list is Elysium's own
//! priority order, and a credential that runs out of quota hands the turn to the one
//! behind it.
//!
//! One list can only hold endpoints of a single request shape, because failover relays
//! the harness's request body unchanged. Claude and `ChatGPT` credentials therefore never
//! appear in the same stack: the highest priority usable credential decides the family,
//! and the rest of that family follows it.

use arsox_sdk::proto::common::v1::Duration;
use arsox_sdk::proto::common::v1::Secret;
use arsox_sdk::proto::harness::v1::Harness;
use arsox_sdk::proto::settings::v1::{
    LlmAuth, ModelEndpoint, OAuthCredential, RetryPolicy, llm_auth,
};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use tracing::{Level, event};

use crate::models::llm::{Llm, LlmType};

/// What the satellite records this endpoint as serving. The harness CLI still chooses
/// the model it asks for, so this is the label that appears in failover incidents
/// rather than a routing decision.
const CLAUDE_MODEL: &str = "claude-opus-5";
const CODEX_MODEL: &str = "gpt-5-codex";

/// How hard one credential is tried before the next one is.
///
/// The satellite's default is ten attempts spanning about six minutes, which is the
/// right shape for a rate limit that clears on its own. This stack exists for the other
/// case: a credential whose quota or credits are spent answers the same way for hours,
/// and every wait is time the credential behind it would have served. Three attempts
/// keep a momentary burst from costing a failover, and anything longer rolls over.
///
/// A provider that rejects the credential outright, or refuses with any status outside
/// the retry set, is never retried and rolls over at once.
const ENDPOINT_ATTEMPTS: u32 = 3;
const ENDPOINT_INITIAL_BACKOFF_SECONDS: i64 = 5;

/// Also the ceiling a provider's `Retry-After` is clamped to, so a header asking for
/// hours cannot hold the turn on a credential that is spent.
const ENDPOINT_MAX_BACKOFF_SECONDS: i64 = 15;

/// `OpenAI` endpoints are declared as an origin: the proxy forwards the path the harness
/// asked for, so a versioned prefix here would arrive as `/v1/v1/responses`. Anthropic's
/// default needs no entry at all.
const OPENAI_BASE_URL: &str = "https://api.openai.com";

/// Which request shape a credential speaks, and so which harness it can drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Claude,
    Codex,
}

impl Family {
    const fn of(type_: LlmType) -> Self {
        match type_ {
            LlmType::ClaudeApiToken | LlmType::ClaudeCodeOauth => Self::Claude,
            LlmType::ChatgptApiToken | LlmType::ChatgptOauth => Self::Codex,
        }
    }

    const fn harness(self) -> Harness {
        match self {
            Self::Claude => Harness::Claude,
            Self::Codex => Harness::Codex,
        }
    }

    const fn model(self) -> &'static str {
        match self {
            Self::Claude => CLAUDE_MODEL,
            Self::Codex => CODEX_MODEL,
        }
    }

    fn base_url(self) -> Option<String> {
        match self {
            Self::Claude => None,
            Self::Codex => Some(OPENAI_BASE_URL.to_owned()),
        }
    }
}

/// The harness a thread runs, and the endpoints it fails over through.
#[derive(Debug)]
pub struct ModelStack {
    pub harness: Harness,
    pub endpoints: Vec<ModelEndpoint>,
}

/// What the Codex CLI writes to `~/.codex/auth.json`. Elysium stores the whole file,
/// because that is what a person can copy without knowing which field matters.
#[derive(Debug, Deserialize)]
struct CodexAuthFile {
    #[serde(rename = "OPENAI_API_KEY")]
    api_key: Option<String>,
    tokens: Option<CodexTokens>,
}

#[derive(Debug, Deserialize)]
struct CodexTokens {
    access_token: Option<String>,
    refresh_token: Option<String>,
}

/// Builds the stack from credentials already ordered by priority, then age.
///
/// Returns `None` when no credential is usable, which leaves the thread declaring no
/// endpoint. The satellite then falls back to whatever credential it holds itself.
pub fn build(credentials: &[(Llm, SecretString)], now: DateTime<Utc>) -> Option<ModelStack> {
    let mut family: Option<Family> = None;
    let mut endpoints = Vec::new();

    for (llm, token) in credentials {
        if !llm.is_active || llm.expires_at.is_some_and(|expires_at| expires_at <= now) {
            continue;
        }

        // The first usable credential decides the shape; later ones must match it.
        let credential_family = Family::of(llm.type_);
        if *family.get_or_insert(credential_family) != credential_family {
            continue;
        }

        let Some(auth) = authenticate(llm, token) else {
            event!(
                name: "coding_session.credential.unusable",
                Level::WARN,
                llm.id = %llm.id,
                llm.name = %llm.name,
                "a credential could not be shaped into an endpoint and was skipped",
            );
            continue;
        };

        endpoints.push(ModelEndpoint {
            name: llm.name.clone(),
            model: credential_family.model().to_owned(),
            base_url: credential_family.base_url(),
            auth: Some(auth),
            retry: Some(rollover_policy()),
        });
    }

    let harness = family?.harness();
    if endpoints.is_empty() {
        return None;
    }

    Some(ModelStack { harness, endpoints })
}

/// How one credential presents itself. `None` when the stored token is not the shape
/// its type promises, such as a Codex file that is not JSON.
fn authenticate(llm: &Llm, token: &SecretString) -> Option<LlmAuth> {
    let credential = match llm.type_ {
        LlmType::ClaudeApiToken | LlmType::ChatgptApiToken => {
            llm_auth::Credential::ApiKey(secret(token.expose_secret()))
        }
        // `claude setup-token` mints a long-lived token, which is a subscription token
        // rather than an API key: the satellite sends it as a bearer.
        LlmType::ClaudeCodeOauth => {
            llm_auth::Credential::SubscriptionToken(secret(token.expose_secret()))
        }
        LlmType::ChatgptOauth => codex_credential(token.expose_secret())?,
    };

    Some(LlmAuth {
        credential: Some(credential),
        // Absent lets the satellite infer the header from the endpoint's base URL,
        // which is right for both providers Elysium talks to.
        presentation: None,
    })
}

/// Reads a stored `auth.json`. A Codex sign-in leaves OAuth tokens; an operator who
/// pasted an API key into the same file is honoured too, since the file has a slot
/// for one.
fn codex_credential(stored: &str) -> Option<llm_auth::Credential> {
    let file: CodexAuthFile = serde_json::from_str(stored).ok()?;

    if let Some(api_key) = file.api_key.filter(|key| !key.trim().is_empty()) {
        return Some(llm_auth::Credential::ApiKey(secret(&api_key)));
    }

    let tokens = file.tokens?;
    let access_token = tokens
        .access_token
        .filter(|token| !token.trim().is_empty())?;

    Some(llm_auth::Credential::Oauth(OAuthCredential {
        access_token: Some(secret(&access_token)),
        // Without a refresh token the satellite cannot renew, and the endpoint fails
        // over once the access token expires.
        refresh_token: tokens
            .refresh_token
            .filter(|token| !token.trim().is_empty())
            .map(|token| secret(&token)),
        // The file records when it was last refreshed, not when the token expires.
        expires_at: None,
    }))
}

/// When to stop waiting on one credential and try the next.
///
/// `retry_on_status` is left empty, which keeps the satellite's own set: 429, the rate
/// and usage limit, and 529, Anthropic's overload. Both are the provider saying "not
/// now"; everything else it says is a reason to move on immediately.
fn rollover_policy() -> RetryPolicy {
    RetryPolicy {
        max_attempts: Some(ENDPOINT_ATTEMPTS),
        initial_backoff: Some(Duration {
            seconds: ENDPOINT_INITIAL_BACKOFF_SECONDS,
            nanos: 0,
        }),
        max_backoff: Some(Duration {
            seconds: ENDPOINT_MAX_BACKOFF_SECONDS,
            nanos: 0,
        }),
        retry_on_status: Vec::new(),
    }
}

fn secret(value: &str) -> Secret {
    Secret {
        value: Some(value.to_owned()),
        display: None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use uuid::Uuid;

    use super::*;

    fn credential(name: &str, type_: LlmType, token: &str) -> (Llm, SecretString) {
        let llm = Llm {
            id: Uuid::now_v7(),
            name: name.to_owned(),
            description: String::new(),
            type_,
            secret_token_encrypted: Vec::new(),
            priority: 0,
            is_active: true,
            expires_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        (llm, SecretString::from(token.to_owned()))
    }

    fn tokens_of(endpoint: &ModelEndpoint) -> Option<llm_auth::Credential> {
        endpoint.auth.as_ref()?.credential.clone()
    }

    #[test]
    fn the_stack_keeps_priority_order_and_one_request_shape() {
        let credentials = vec![
            credential("Personal", LlmType::ClaudeCodeOauth, "sk-ant-oat01-first"),
            credential(
                "Fallback key",
                LlmType::ClaudeApiToken,
                "sk-ant-api03-second",
            ),
            credential("Codex", LlmType::ChatgptApiToken, "sk-openai"),
        ];

        let stack = build(&credentials, Utc::now()).expect("a usable credential");

        assert_eq!(stack.harness, Harness::Claude);
        let names: Vec<&str> = stack
            .endpoints
            .iter()
            .map(|endpoint| endpoint.name.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["Personal", "Fallback key"],
            "the ChatGPT credential speaks another request shape"
        );
        assert!(
            stack.endpoints[0].base_url.is_none(),
            "Anthropic's default base URL needs no entry"
        );
        assert_eq!(
            tokens_of(&stack.endpoints[0]),
            Some(llm_auth::Credential::SubscriptionToken(secret(
                "sk-ant-oat01-first"
            ))),
            "a setup-token is a subscription token, not an API key"
        );
        assert_eq!(
            tokens_of(&stack.endpoints[1]),
            Some(llm_auth::Credential::ApiKey(secret("sk-ant-api03-second")))
        );
    }

    #[test]
    fn a_spent_credential_rolls_over_rather_than_waiting_out_its_quota() {
        let credentials = vec![credential(
            "Primary",
            LlmType::ClaudeCodeOauth,
            "sk-ant-oat01",
        )];

        let stack = build(&credentials, Utc::now()).expect("a usable credential");

        let retry = stack.endpoints[0].retry.clone().expect("a declared policy");
        assert_eq!(
            retry.max_attempts,
            Some(ENDPOINT_ATTEMPTS),
            "the satellite's ten attempts would wait out a quota that lasts hours"
        );
        assert_eq!(
            retry.max_backoff.map(|backoff| backoff.seconds),
            Some(ENDPOINT_MAX_BACKOFF_SECONDS),
            "a provider's Retry-After is clamped to this"
        );
        assert!(
            retry.retry_on_status.is_empty(),
            "an empty set keeps the satellite's 429 and 529"
        );
    }

    #[test]
    fn inactive_and_expired_credentials_are_left_out() {
        let now = Utc::now();
        let (mut inactive, inactive_token) =
            credential("Inactive", LlmType::ClaudeApiToken, "sk-ant-inactive");
        inactive.is_active = false;
        let (mut expired, expired_token) =
            credential("Expired", LlmType::ClaudeApiToken, "sk-ant-expired");
        expired.expires_at = Some(now - TimeDelta::seconds(1));
        let (mut live, live_token) = credential("Live", LlmType::ClaudeApiToken, "sk-ant-live");
        live.expires_at = Some(now + TimeDelta::seconds(1));

        let credentials = vec![
            (inactive, inactive_token),
            (expired, expired_token),
            (live, live_token),
        ];
        let stack = build(&credentials, now).expect("one credential is live");

        assert_eq!(stack.endpoints.len(), 1);
        assert_eq!(stack.endpoints[0].name, "Live");
    }

    #[test]
    fn a_codex_family_stack_declares_openais_origin_and_reads_auth_json() {
        let auth_json = r#"{
            "OPENAI_API_KEY": null,
            "tokens": { "access_token": "access-1", "refresh_token": "refresh-1" }
        }"#;
        let credentials = vec![credential("Codex", LlmType::ChatgptOauth, auth_json)];

        let stack = build(&credentials, Utc::now()).expect("a usable credential");

        assert_eq!(stack.harness, Harness::Codex);
        assert_eq!(
            stack.endpoints[0].base_url.as_deref(),
            Some(OPENAI_BASE_URL),
            "the origin, so the proxy's forwarded path is not doubled"
        );
        assert_eq!(
            tokens_of(&stack.endpoints[0]),
            Some(llm_auth::Credential::Oauth(OAuthCredential {
                access_token: Some(secret("access-1")),
                refresh_token: Some(secret("refresh-1")),
                expires_at: None,
            }))
        );
    }

    #[test]
    fn a_codex_file_that_is_not_json_is_skipped_rather_than_sent() {
        let credentials = vec![
            credential("Pasted the wrong thing", LlmType::ChatgptOauth, "sk-proj-1"),
            credential("Working key", LlmType::ChatgptApiToken, "sk-openai"),
        ];

        let stack = build(&credentials, Utc::now()).expect("the second credential is usable");

        assert_eq!(stack.endpoints.len(), 1);
        assert_eq!(stack.endpoints[0].name, "Working key");
    }

    #[test]
    fn no_usable_credential_declares_no_endpoint() {
        let (mut inactive, token) = credential("Inactive", LlmType::ClaudeApiToken, "sk-ant");
        inactive.is_active = false;

        assert!(build(&[(inactive, token)], Utc::now()).is_none());
        assert!(build(&[], Utc::now()).is_none());
    }
}
