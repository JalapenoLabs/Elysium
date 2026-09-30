// Copyright © 2026 Jalapeno Labs

//! `POST /internal/kratos/registration`: whether a sign-up may proceed, asked by Kratos
//! before it stores the identity (`kratos/kratos.yml`).
//!
//! A sign-up is refused when an admin closed sign-up (unless nobody has signed up yet), or
//! when it uses a passkey: every account starts with a password, and passkeys are added
//! afterwards. The refusal is shown on the form, under the email field.
//!
//! The account itself is not created here: `crate::auth::middleware` creates it on the
//! person's first request, so a lost webhook can never leave an identity without one.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;
use tracing::{Level, event};

use crate::errors::ApiError;
use crate::models::user;
use crate::state::AppState;

/// Kratos message ids for the two refusals, in a range Kratos does not use, so the frontend
/// can show its own translation of each.
const SIGNUP_CLOSED_MESSAGE_ID: u32 = 4_190_001;
const PASSWORD_REQUIRED_MESSAGE_ID: u32 = 4_190_002;

/// How the person is signing up, from `kratos/registration*.jsonnet`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Method {
    Password,
    Passkey,
}

#[derive(Debug, Deserialize)]
pub struct RequestBody {
    method: Method,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;

    if matches!(body.method, Method::Passkey) {
        return Ok(refuse(
            PASSWORD_REQUIRED_MESSAGE_ID,
            "Sign up with a password. You can add a passkey once your account exists.",
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let open = user::workspace_settings(&mut connection).await?.signup_open;
    if !open && user::has_people(&mut connection).await? {
        event!(
            name: "auth.signup.refused",
            Level::INFO,
            "refused a sign-up while sign-up is closed",
        );
        return Ok(refuse(
            SIGNUP_CLOSED_MESSAGE_ID,
            "Sign-up is closed. Ask an admin to open it.",
        ));
    }

    Ok(StatusCode::NO_CONTENT.into_response())
}

/// The answer Kratos reads as "refuse, and show this under the email field".
fn refuse(id: u32, text: &str) -> Response {
    let body = json!({
        "messages": [{
            "instance_ptr": "#/traits/email",
            "messages": [{ "id": id, "text": text, "type": "error" }],
        }],
    });
    (StatusCode::FORBIDDEN, Json(body)).into_response()
}
