// Copyright © 2026 Jalapeno Labs

//! `/api/v1/environment-variables`: the global environment variables every coding
//! session's satellite thread receives.
//!
//! A secret variable's value is write-only over HTTP, and a non-secret one's is returned as
//! plaintext. Every value is sealed in storage either way.

mod create_environment_variable;
mod delete_environment_variable;
mod get_environment_variable;
mod list_environment_variables;
mod update_environment_variable;

use anyhow::Context;
use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::crypto::Cipher;
use crate::environment::key_refusal;
use crate::errors::ApiError;
use crate::models::environment_variable::EnvironmentVariable;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_environment_variables::handle).post(create_environment_variable::handle),
        )
        .route(
            "/{id}",
            get(get_environment_variable::handle)
                .patch(update_environment_variable::handle)
                .delete(delete_environment_variable::handle),
        )
}

/// Upper bound on a stored value, 32 KiB. Enough for a certificate chain or a small config
/// file, while keeping every thread's environment, which is sent to the satellite whole,
/// far below what a process accepts.
const VALUE_MAX_BYTES: usize = 32 * 1024;

/// Refused whenever a secret would end up with an empty value. The satellite scrubs a
/// secret's value from thread output, and scrubbing the empty string has nothing to match.
/// A blank secret field in the editor also means "keep the stored value", so an empty
/// secret is never what someone meant.
const EMPTY_SECRET_MESSAGE: &str = "a secret variable needs a value";

/// A variable as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariableResponse {
    id: Uuid,
    key: String,
    is_secret: bool,
    description: String,
    /// The plaintext of a non-secret variable, or `null` for a secret one, whose value is
    /// never returned.
    value: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl EnvironmentVariableResponse {
    /// Builds the response, decrypting the value only when the variable is not secret.
    ///
    /// # Errors
    /// Fails when a non-secret value will not open, which only a changed encryption key or
    /// an altered row can cause.
    pub fn new(variable: EnvironmentVariable, cipher: &Cipher) -> anyhow::Result<Self> {
        let value = if variable.is_secret {
            None
        } else {
            let plaintext = variable.value(cipher).with_context(|| {
                format!(
                    "environment variable {} could not be decrypted",
                    variable.key
                )
            })?;
            Some(plaintext.expose_secret().to_owned())
        };

        Ok(Self {
            id: variable.id,
            key: variable.key,
            is_secret: variable.is_secret,
            description: variable.description,
            value,
            created_at: variable.created_at,
            updated_at: variable.updated_at,
        })
    }
}

/// Refuses a key that cannot be stored, naming the key and the rule it breaks.
fn check_key(key: &str) -> Result<(), ApiError> {
    if let Some(refusal) = key_refusal(key) {
        return Err(ApiError::BadRequest(format!("{key} {}", refusal.reason())));
    }
    Ok(())
}

/// A value from a request body, bounded as it is parsed.
///
/// Parsed as a secret whatever the variable's flag, so a value never reaches a validation
/// report or debug output. It is never trimmed: leading and trailing whitespace, including
/// the newline that ends a PEM block, can be part of a value.
#[derive(Debug)]
pub struct VariableValue(pub SecretString);

impl<'de> Deserialize<'de> for VariableValue {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() > VALUE_MAX_BYTES {
            return Err(serde::de::Error::custom("value exceeds 32 KiB"));
        }
        Ok(Self(SecretString::from(value)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_keep_their_whitespace_and_are_bounded_while_parsing() {
        let parsed: VariableValue =
            serde_json::from_value(serde_json::json!("  padded\n")).expect("parses");
        assert_eq!(parsed.0.expose_secret(), "  padded\n");

        let empty: VariableValue = serde_json::from_value(serde_json::json!("")).expect("parses");
        assert_eq!(empty.0.expose_secret(), "");

        serde_json::from_value::<VariableValue>(serde_json::json!("v".repeat(VALUE_MAX_BYTES)))
            .expect("exactly 32 KiB");
        serde_json::from_value::<VariableValue>(serde_json::json!("v".repeat(VALUE_MAX_BYTES + 1)))
            .expect_err("an oversized value");

        assert!(!format!("{parsed:?}").contains("padded"));
    }

    #[test]
    fn refused_keys_name_the_key_and_the_rule() {
        let Err(ApiError::BadRequest(message)) = check_key("GITHUB_TOKEN") else {
            panic!("GITHUB_TOKEN is reserved");
        };
        assert!(message.starts_with("GITHUB_TOKEN is reserved for Elysium"));

        check_key("NPM_TOKEN").expect("an ordinary key");
    }
}
