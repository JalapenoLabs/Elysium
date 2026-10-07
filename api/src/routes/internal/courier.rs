// Copyright © 2026 Jalapeno Labs

//! `POST /internal/kratos/courier`: a message Kratos would email. Elysium has no mail for
//! this yet, so a recovery link is logged for an operator to pass on, and anything else is
//! noted and dropped. See `docs/auth.md`.

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use tracing::{Level, event};

use crate::errors::ApiError;

/// A message, as `kratos/courier.jsonnet` shapes it.
#[derive(Debug, Deserialize)]
pub struct RequestBody {
    recipient: String,
    template_type: String,
    #[serde(default)]
    recovery_url: Option<String>,
    #[serde(default)]
    expires_in_minutes: Option<u32>,
}

pub async fn handle(
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;

    let Some(recovery_url) = body
        .recovery_url
        .filter(|_| body.template_type == "recovery_valid")
    else {
        // Recovery for an address with no account sends nothing (`notify_unknown_recipients`
        // is off), and verification is off, so nothing else is expected here.
        event!(
            name: "auth.courier.dropped",
            Level::INFO,
            template = body.template_type,
            "dropped a message Kratos would have emailed",
        );
        return Ok(StatusCode::NO_CONTENT);
    };

    // Deliberately at WARN with the address: this line is how the link reaches its owner
    // until Elysium sends mail, so an operator must be able to find it.
    event!(
        name: "auth.recovery.issued",
        Level::WARN,
        user.email = body.recipient,
        url.full = recovery_url,
        expires_in_minutes = body.expires_in_minutes.unwrap_or_default(),
        "recovery link for {{user.email}}: {{url.full}}",
    );
    Ok(StatusCode::NO_CONTENT)
}
