// Copyright © 2026 Jalapeno Labs

//! `/api/v1/llms`: LLM provider credentials. Tokens are write-only over HTTP.

mod create_llm;
mod delete_llm;
mod get_llm;
mod list_llms;
mod update_llm;

use axum::Router;
use axum::routing::get;
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::models::llm::{Llm, LlmType};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_llms::handle).post(create_llm::handle))
        .route(
            "/{id}",
            get(get_llm::handle)
                .patch(update_llm::handle)
                .delete(delete_llm::handle),
        )
}

/// Upper bound on a stored token. OAuth refresh bundles run a few kilobytes; this
/// leaves headroom while keeping one row from carrying an arbitrary blob.
const SECRET_TOKEN_MAX_BYTES: usize = 16 * 1024;

/// A credential as clients see it. The token is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmResponse {
    id: Uuid,
    name: String,
    description: String,
    #[serde(rename = "type")]
    llm_type: LlmType,
    priority: i32,
    is_active: bool,
    expires_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Llm> for LlmResponse {
    fn from(llm: Llm) -> Self {
        Self {
            id: llm.id,
            name: llm.name,
            description: llm.description,
            llm_type: llm.type_,
            priority: llm.priority,
            is_active: llm.is_active,
            expires_at: llm.expires_at,
            created_at: llm.created_at,
            updated_at: llm.updated_at,
        }
    }
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// A token from a request body, checked as it is parsed.
///
/// Validation happens during deserialization rather than through `validator`,
/// because a validator error report would need to serialize the value itself.
/// The plaintext moves straight into a [`SecretString`], which zeroizes on drop,
/// so a rejected token is wiped as soon as the error is returned.
#[derive(Debug)]
pub struct SecretToken(pub SecretString);

impl<'de> Deserialize<'de> for SecretToken {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let token = SecretString::from(String::deserialize(deserializer)?);

        if token.expose_secret().trim().is_empty() {
            return Err(serde::de::Error::custom("secretToken must not be blank"));
        }
        if token.expose_secret().len() > SECRET_TOKEN_MAX_BYTES {
            return Err(serde::de::Error::custom("secretToken exceeds 16 KiB"));
        }

        Ok(Self(token))
    }
}
