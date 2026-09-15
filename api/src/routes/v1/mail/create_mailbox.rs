// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/mail/accounts`: create a mailbox on one of the mail server's domains.
//!
//! Gmail and Outlook accounts are not created here; they arrive through the OAuth
//! flow. A self-hosted mailbox gets a generated password nobody ever sees: Elysium is
//! its only client.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;
use validator::{Validate, ValidateEmail, ValidationError};

use super::{MailAccountResponse, require_administrator};
use crate::errors::ApiError;
use crate::mail::broker::random_token;
use crate::models::mail_account::{self, MailAccountKind, NewMailAccount};
use crate::models::mail_domain;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 64), custom(function = "validate_local_part"))]
    local_part: String,
    /// One of `GET /api/v1/mail/domains`.
    domain_id: Uuid,
    #[serde(default)]
    #[validate(length(max = 120))]
    display_name: String,
}

/// Letters, digits, and `. _ + -`, without a leading, trailing, or doubled dot.
fn validate_local_part(value: &str) -> Result<(), ValidationError> {
    let allowed = value
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character));
    if !allowed || value.starts_with('.') || value.ends_with('.') || value.contains("..") {
        return Err(ValidationError::new("local_part")
            .with_message("use letters, digits, and . _ + - only".into()));
    }
    Ok(())
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let administrator = require_administrator(
        &state,
        "a mail server must be created before mailboxes can be created",
    )
    .await?;
    let stalwart = &state.mail.stalwart;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let domain = match mail_domain::find(&mut connection, body.domain_id).await {
        Err(diesel::result::Error::NotFound) => {
            return Err(ApiError::BadRequest(
                "the mail domain does not exist".to_owned(),
            ));
        }
        found => found?,
    };

    let local_part = body.local_part.to_lowercase();
    let address = format!("{local_part}@{}", domain.name);
    if !address.validate_email() {
        return Err(ApiError::BadRequest(format!(
            "{address} is not a valid address"
        )));
    }

    if mail_account::find_by_address(&mut connection, &address)
        .await?
        .is_some()
    {
        return Err(ApiError::Conflict(
            "a mailbox with that address is already connected",
        ));
    }
    drop(connection);

    let password = SecretString::from(random_token());
    let stalwart_id = stalwart
        .create_mailbox(&administrator, &local_part, &domain.stalwart_id, &password)
        .await?;

    let new_account = NewMailAccount {
        kind: MailAccountKind::SelfHosted,
        address,
        display_name: body.display_name.trim().to_owned(),
        credential: password,
        external_id: Some(stalwart_id.clone()),
        mail_domain_id: Some(domain.id),
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let created = match mail_account::create(&mut connection, &state.cipher, &new_account).await {
        Ok(account) => account,
        Err(error) => {
            // Without the row nothing knows the password, so the mailbox would be
            // unreachable and would block its address. Remove it again.
            if let Err(cleanup) = stalwart.destroy_mailbox(&administrator, &stalwart_id).await {
                event!(
                    name: "mail.mailbox.orphaned",
                    Level::ERROR,
                    mail.stalwart.account_id = %stalwart_id,
                    error.message = %cleanup,
                    "a mailbox was created in Stalwart but not recorded, and could not be removed",
                );
            }
            return Err(error.into());
        }
    };

    state
        .events
        .publish(&ServerEvent::MailAccountUpserted(created.clone().into()));
    Ok((
        StatusCode::CREATED,
        Json(json!({ "account": MailAccountResponse::from(created) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOMAIN_ID: &str = "01a0a5e5-a7d9-7774-9f90-39274b7d16e0";

    fn parse(body: Value) -> RequestBody {
        serde_json::from_value(body).expect("parses")
    }

    #[test]
    fn a_plain_mailbox_request_is_valid() {
        parse(json!({ "localPart": "agent.one", "domainId": DOMAIN_ID }))
            .validate()
            .expect("valid");
    }

    #[test]
    fn local_parts_are_checked() {
        for local_part in ["agent one", ".agent", "agent..one", "agent."] {
            let errors = parse(json!({ "localPart": local_part, "domainId": DOMAIN_ID }))
                .validate()
                .expect_err("invalid");
            assert!(
                errors.field_errors().contains_key("local_part"),
                "{local_part} should be refused"
            );
        }
    }
}
