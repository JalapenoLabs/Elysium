// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/mail/server`: finish the bundled mail server's setup.
//!
//! A new Stalwart prints a temporary administrator password once, to its log. The
//! operator pastes it here with the domain the server should use; Stalwart then issues
//! a permanent administrator, which is sealed in Postgres before anything else happens,
//! because Stalwart shows it only once and the temporary password stops working. The
//! request returns once Stalwart has restarted and accepts the new administrator.
//!
//! Setting up a server that was reset (its volume removed) replaces the stored
//! administrator. A server that is already set up refuses the temporary password.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use secrecy::SecretString;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};
use validator::Validate;

use super::{mail_server_status, validate_domain};
use crate::errors::ApiError;
use crate::mail::stalwart::StalwartError;
use crate::models::mail_server::{self, NewMailServer};
use crate::state::AppState;

#[derive(Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 3, max = 253), custom(function = "validate_domain"))]
    domain: String,
    /// Stalwart generates it; the bound only keeps a pasted log line from being sent on.
    #[validate(length(min = 1, max = 256))]
    bootstrap_password: String,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    body.validate()?;
    let domain = body.domain.to_lowercase();
    let bootstrap_password = SecretString::from(body.bootstrap_password.trim().to_owned());

    // Held from before setup starts: once Stalwart issues the administrator, a database
    // that cannot be reached would lose it for good.
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let stalwart = &state.mail.stalwart;
    let administrator = stalwart
        .complete_setup(&bootstrap_password, &domain)
        .await
        .map_err(|error| match error {
            StalwartError::Unauthorized => ApiError::BadRequest(
                "the mail server did not accept that temporary password, or it is already set up"
                    .to_owned(),
            ),
            other => other.into(),
        })?;

    let new_server = NewMailServer {
        domain,
        admin_username: administrator.username.clone(),
        admin_secret: administrator.password.clone(),
    };
    if let Err(error) = mail_server::replace(&mut connection, &state.cipher, &new_server).await {
        event!(
            name: "mail.server.administrator_lost",
            Level::ERROR,
            error.message = %error,
            "the mail server issued an administrator that could not be stored; remove its volumes and set it up again",
        );
        return Err(error.into());
    }
    drop(connection);

    stalwart.wait_until_serving(&administrator).await?;
    event!(
        name: "mail.server.set_up",
        Level::INFO,
        mail.server.domain = %new_server.domain,
        "the mail server is set up",
    );

    let server = mail_server_status(&state).await?;
    Ok(Json(json!({ "server": server })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: Value) -> RequestBody {
        serde_json::from_value(body).expect("parses")
    }

    #[test]
    fn a_domain_and_password_are_required() {
        parse(json!({ "domain": "elysium.local", "bootstrapPassword": "Gt0vA6h90qQO26RX" }))
            .validate()
            .expect("valid");

        let errors = parse(json!({ "domain": "localhost", "bootstrapPassword": "" }))
            .validate()
            .expect_err("invalid");
        assert!(errors.field_errors().contains_key("domain"));
        assert!(errors.field_errors().contains_key("bootstrap_password"));
    }

    #[test]
    fn unknown_fields_are_refused() {
        let parsed = serde_json::from_value::<RequestBody>(json!({
            "domain": "elysium.local",
            "bootstrapPassword": "x",
            "adminPassword": "chosen"
        }));
        assert!(parsed.is_err());
    }
}
