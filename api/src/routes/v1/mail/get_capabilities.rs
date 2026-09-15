// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/capabilities`: which kinds of mailbox this deployment can connect.
//!
//! Asks the broker and the mail server live rather than trusting configuration, so the
//! settings page can tell "not configured" or "not set up" apart from "unreachable".

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::mail_server_status;
use crate::errors::ApiError;
use crate::models::mail_account::MailAccountKind;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let (oauth_kinds, broker_error) = match &state.mail.broker {
        None => (Vec::new(), None),
        Some(broker) => match broker.available_kinds().await {
            Ok(kinds) => (kinds, None),
            Err(error) => (Vec::<MailAccountKind>::new(), Some(error.to_string())),
        },
    };
    let mail_server = mail_server_status(&state).await?;

    Ok(Json(json!({
        "capabilities": {
            "brokerConfigured": state.mail.broker.is_some(),
            "brokerError": broker_error,
            "oauthKinds": oauth_kinds,
            "mailServer": mail_server,
        }
    })))
}
