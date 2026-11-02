// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/llms/claude-sign-ins/{id}`: finish a sign-in with the code Anthropic showed.
//!
//! The account becomes a `claude-account` credential named after its email unless a name is
//! given. An account signed in before keeps its credential, name, and priority, and takes the
//! new tokens, which also clears an expired sign-in.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{LlmResponse, SecretToken, sign_in_error};
use crate::auth::CurrentUser;
use crate::claude_accounts::FinishError;
use crate::errors::ApiError;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// The code Anthropic's page showed, which the CLI trades for the account's tokens.
    code: SecretToken,
    #[validate(length(min = 1, max = 120))]
    name: Option<String>,
    #[serde(default)]
    priority: i32,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let saved = state
        .claude_accounts
        .finish_sign_in(
            id,
            current.id(),
            &body.code.0,
            body.name.as_deref(),
            body.priority,
        )
        .await
        .map_err(|error| match error {
            FinishError::SignIn(error) => sign_in_error(error),
            FinishError::Profile(error) => ApiError::BadGateway(error.to_string()),
            FinishError::Database(error) => ApiError::from(error),
            FinishError::NoDatabase(reason) => ApiError::Internal(anyhow::anyhow!(reason)),
        })?;

    let status = if saved.is_new {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };
    state
        .events
        .publish(&ServerEvent::LlmUpserted(LlmResponse::new(
            saved.llm.clone(),
            Some(&saved.account),
        )));
    Ok((
        status,
        Json(json!({ "llm": LlmResponse::new(saved.llm, Some(&saved.account)) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_needs_a_code_and_refuses_strays() {
        let body: RequestBody =
            serde_json::from_value(json!({ "code": "abc#state" })).expect("parses");
        body.validate().expect("valid");
        assert_eq!(body.priority, 0);

        serde_json::from_value::<RequestBody>(json!({ "name": "x" }))
            .expect_err("a code is required");
        serde_json::from_value::<RequestBody>(json!({ "code": "abc", "token": "x" }))
            .expect_err("unknown fields are refused");
        serde_json::from_value::<RequestBody>(json!({ "code": "  " }))
            .expect_err("a blank code is refused while parsing");
    }

    #[test]
    fn debug_output_never_reveals_the_code() {
        let body: RequestBody =
            serde_json::from_value(json!({ "code": "very-secret-code" })).expect("parses");
        assert!(!format!("{body:?}").contains("very-secret-code"));
    }
}
