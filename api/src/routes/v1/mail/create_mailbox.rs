// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/mail/accounts`: create a mailbox on the bundled mail server.
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
use validator::{Validate, ValidateEmail, ValidationError};

use super::MailAccountResponse;
use crate::errors::ApiError;
use crate::mail::broker::random_token;
use crate::models::mail_account::{self, MailAccountKind, NewMailAccount};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 64), custom(function = "validate_local_part"))]
    local_part: String,
    #[validate(length(min = 3, max = 253), custom(function = "validate_domain"))]
    domain: String,
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

/// Dot-separated labels of letters, digits, and hyphens, at least two of them.
fn validate_domain(value: &str) -> Result<(), ValidationError> {
    let labels: Vec<&str> = value.split('.').collect();
    let well_formed = labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '-')
        });
    if !well_formed {
        return Err(ValidationError::new("domain")
            .with_message("must be a domain such as example.com".into()));
    }
    Ok(())
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let Some(stalwart) = state.mail.stalwart.as_ref() else {
        return Err(ApiError::Unavailable(
            "self-hosted mail is not configured on this deployment",
        ));
    };

    let local_part = body.local_part.to_lowercase();
    let domain = body.domain.to_lowercase();
    let address = format!("{local_part}@{domain}");
    if !address.validate_email() {
        return Err(ApiError::BadRequest(format!(
            "{address} is not a valid address"
        )));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
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
        .create_mailbox(&local_part, &domain, &password)
        .await?;

    let new_account = NewMailAccount {
        kind: MailAccountKind::SelfHosted,
        address,
        display_name: body.display_name.trim().to_owned(),
        credential: password,
        external_id: Some(stalwart_id.clone()),
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
            if let Err(cleanup) = stalwart.destroy_mailbox(&stalwart_id).await {
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

    fn parse(body: Value) -> RequestBody {
        serde_json::from_value(body).expect("parses")
    }

    #[test]
    fn a_plain_mailbox_request_is_valid() {
        parse(json!({ "localPart": "agent.one", "domain": "elysium.local" }))
            .validate()
            .expect("valid");
    }

    #[test]
    fn local_parts_and_domains_are_checked() {
        for (local_part, domain, field) in [
            ("agent one", "elysium.local", "local_part"),
            (".agent", "elysium.local", "local_part"),
            ("agent..one", "elysium.local", "local_part"),
            ("agent", "localhost", "domain"),
            ("agent", "-bad.example", "domain"),
            ("agent", "exa mple.com", "domain"),
        ] {
            let errors = parse(json!({ "localPart": local_part, "domain": domain }))
                .validate()
                .expect_err("invalid");
            assert!(
                errors.field_errors().contains_key(field),
                "{local_part}@{domain} should fail on {field}"
            );
        }
    }
}
