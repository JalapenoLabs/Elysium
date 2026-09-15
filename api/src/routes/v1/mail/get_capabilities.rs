// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/mail/capabilities`: whether Gmail and Outlook accounts can connect.
//!
//! Asks the broker live rather than trusting configuration, so the settings page can
//! tell "no broker configured" apart from "broker configured but unreachable". Whether
//! self-hosted mailboxes can be created is the mail server's status, `GET /server`.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::models::mail_account::MailAccountKind;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Json<Value> {
    let (oauth_kinds, broker_error) = match &state.mail.broker {
        None => (Vec::new(), None),
        Some(broker) => match broker.available_kinds().await {
            Ok(kinds) => (kinds, None),
            Err(error) => (Vec::<MailAccountKind>::new(), Some(error.to_string())),
        },
    };

    Json(json!({
        "capabilities": {
            "brokerConfigured": state.mail.broker.is_some(),
            "brokerError": broker_error,
            "oauthKinds": oauth_kinds,
        }
    }))
}
