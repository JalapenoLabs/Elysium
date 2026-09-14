// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/llms`: store a new credential, sealing its token.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{LlmResponse, SecretToken, validate_not_blank};
use crate::errors::ApiError;
use crate::models::llm::{self, LlmType, NewLlm};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: String,
    #[serde(default)]
    #[validate(length(max = 2000))]
    description: String,
    #[serde(rename = "type")]
    llm_type: LlmType,
    secret_token: SecretToken,
    #[serde(default)]
    priority: i32,
    #[serde(default = "default_is_active")]
    is_active: bool,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
}

const fn default_is_active() -> bool {
    true
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let new_llm = NewLlm {
        name: body.name,
        description: body.description,
        type_: body.llm_type,
        secret_token: body.secret_token.0,
        priority: body.priority,
        is_active: body.is_active,
        expires_at: body.expires_at,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let llm = llm::create(&mut connection, &state.cipher, &new_llm).await?;

    state
        .events
        .publish(&ServerEvent::LlmUpserted(LlmResponse::from(llm.clone())));
    Ok((
        StatusCode::CREATED,
        Json(json!({ "llm": LlmResponse::from(llm) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: serde_json::Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    #[test]
    fn minimal_body_applies_defaults() {
        let body = parse(
            json!({ "name": "Primary", "type": "claude-api-token", "secretToken": "sk-ant" }),
        )
        .expect("minimal body parses");

        body.validate().expect("minimal body is valid");
        assert_eq!(body.llm_type, LlmType::ClaudeApiToken);
        assert_eq!(body.description, "");
        assert_eq!(body.priority, 0);
        assert!(body.is_active);
        assert!(body.expires_at.is_none());
    }

    #[test]
    fn unknown_fields_and_types_are_rejected() {
        let unknown_field =
            json!({ "name": "A", "type": "claude-api-token", "secretToken": "x", "admin": true });
        parse(unknown_field).expect_err("unknown fields are refused");

        let unknown_type = json!({ "name": "A", "type": "gemini", "secretToken": "x" });
        parse(unknown_type).expect_err("unknown types are refused");
    }

    #[test]
    fn expires_at_must_carry_an_explicit_offset() {
        let naive = json!({ "name": "A", "type": "chatgpt-oauth", "secretToken": "x", "expiresAt": "2027-01-01T00:00:00" });
        parse(naive).expect_err("a timestamp without an offset is ambiguous and must be refused");

        let offset = json!({ "name": "A", "type": "chatgpt-oauth", "secretToken": "x", "expiresAt": "2027-01-01T02:00:00+02:00" });
        let body = parse(offset).expect("offset timestamps parse");
        assert_eq!(
            body.expires_at.expect("present").to_rfc3339(),
            "2027-01-01T00:00:00+00:00"
        );
    }

    #[test]
    fn blank_and_oversized_names_fail_validation() {
        let blank =
            parse(json!({ "name": "   ", "type": "chatgpt-api-token", "secretToken": "x" }))
                .expect("parses");
        assert!(
            blank
                .validate()
                .expect_err("blank name is invalid")
                .field_errors()
                .contains_key("name")
        );

        let long = parse(
            json!({ "name": "A".repeat(121), "type": "chatgpt-api-token", "secretToken": "x" }),
        )
        .expect("parses");
        assert!(
            long.validate()
                .expect_err("121-char name is invalid")
                .field_errors()
                .contains_key("name")
        );

        // Limits count characters, as Postgres char_length does, not bytes.
        let multibyte = parse(
            json!({ "name": "é".repeat(120), "type": "chatgpt-api-token", "secretToken": "x" }),
        )
        .expect("parses");
        multibyte
            .validate()
            .expect("120 multibyte characters are within the limit");
    }

    #[test]
    fn blank_and_oversized_tokens_are_rejected_while_parsing() {
        let blank = parse(json!({ "name": "A", "type": "chatgpt-api-token", "secretToken": "  " }));
        assert!(
            blank
                .expect_err("blank token")
                .to_string()
                .contains("must not be blank")
        );

        let at_limit = parse(
            json!({ "name": "A", "type": "chatgpt-api-token", "secretToken": "x".repeat(16 * 1024) }),
        );
        at_limit.expect("a token exactly at the limit is accepted");

        let over_limit = parse(
            json!({ "name": "A", "type": "chatgpt-api-token", "secretToken": "x".repeat(16 * 1024 + 1) }),
        );
        assert!(
            over_limit
                .expect_err("oversized token")
                .to_string()
                .contains("exceeds 16 KiB")
        );
    }

    #[test]
    fn debug_output_never_reveals_the_token() {
        let body = parse(
            json!({ "name": "A", "type": "claude-code-oauth", "secretToken": "sk-very-secret" }),
        )
        .expect("parses");
        assert!(!format!("{body:?}").contains("sk-very-secret"));
    }
}
