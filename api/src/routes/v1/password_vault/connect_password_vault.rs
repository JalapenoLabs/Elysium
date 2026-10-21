// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/password-vault`: set up the vault, or replace its credentials.
//!
//! The CLI signs in with the API key and unlocks with the master password before anything is
//! stored, so every stored vault names the account it reaches. A secret left out keeps the
//! stored one, so changing the server or the key does not mean retyping the master password.
//!
//! The chosen collection is kept when the account is the same and still reaches it; otherwise
//! agents read nothing until an admin chooses again.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};
use url::Url;

use super::{PasswordVaultResponse, VaultSecret, vault_error};
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::password_vault::{self, PasswordServer, PasswordVault, VerifiedVault};
use crate::passwords::{Credentials, Server};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// The longest server URL kept, as the column's check allows.
const MAX_SERVER_URL_LENGTH: usize = 2_048;

/// The longest client id kept, as the column's check allows.
const MAX_CLIENT_ID_LENGTH: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    server: PasswordServer,
    /// Required for a self-hosted server, refused otherwise.
    #[serde(default)]
    server_url: Option<String>,
    client_id: String,
    /// Required the first time; absent keeps the stored one.
    #[serde(default)]
    client_secret: Option<VaultSecret>,
    /// Required the first time; absent keeps the stored one.
    #[serde(default)]
    master_password: Option<VaultSecret>,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let server = validate_server(body.server, body.server_url.as_deref())?;
    let client_id = validate_client_id(&body.client_id)?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let stored = password_vault::find(&mut connection).await?;
    drop(connection);

    let client_secret = match (body.client_secret, &stored) {
        (Some(secret), _) => secret.0,
        (None, Some(stored)) => stored
            .client_secret(&state.cipher)
            .context("cannot open the stored client secret")?,
        (None, None) => return Err(ApiError::BadRequest("clientSecret is required".to_owned())),
    };
    let master_password = match (body.master_password, &stored) {
        (Some(secret), _) => secret.0,
        (None, Some(stored)) => stored
            .master_password(&state.cipher)
            .context("cannot open the stored master password")?,
        (None, None) => {
            return Err(ApiError::BadRequest(
                "masterPassword is required".to_owned(),
            ));
        }
    };
    let credentials = Credentials {
        server,
        client_id,
        client_secret,
        master_password,
    };

    let connected = state
        .passwords
        .connect(&credentials)
        .await
        .map_err(vault_error)?;

    let (server, server_url) = credentials.server.stored();
    let verified = VerifiedVault {
        server,
        server_url,
        client_id: credentials.client_id,
        client_secret: credentials.client_secret,
        master_password: credentials.master_password,
        account_email: connected.account_email,
    };
    let kept_collection = stored
        .as_ref()
        .filter(|stored| same_account(stored, &verified))
        .and_then(PasswordVault::collection)
        .filter(|collection| {
            connected
                .collections
                .iter()
                .any(|reachable| reachable.id == collection.id)
        });

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let vault = password_vault::save(
        &mut connection,
        &state.cipher,
        &verified,
        kept_collection.as_ref(),
        admin.id(),
    )
    .await?;

    let response = PasswordVaultResponse::from(&vault);
    state
        .events
        .publish(&ServerEvent::PasswordVaultUpdated(response.clone()));
    Ok(Json(json!({
        "vault": response,
        "status": state.passwords.status(),
        "collections": connected.collections,
    })))
}

fn same_account(stored: &PasswordVault, verified: &VerifiedVault) -> bool {
    stored.server == verified.server
        && stored.server_url == verified.server_url
        && stored.account_email == verified.account_email
}

/// Bitwarden's own servers take no URL. A self-hosted server is an https URL, with a path if
/// it is served under one, and with no credentials, query, or fragment.
fn validate_server(kind: PasswordServer, url: Option<&str>) -> Result<Server, ApiError> {
    let refuse = |reason: &str| ApiError::BadRequest(format!("serverUrl {reason}"));
    match (kind, url.map(str::trim)) {
        (PasswordServer::BitwardenUs, None) => Ok(Server::BitwardenUs),
        (PasswordServer::BitwardenEu, None) => Ok(Server::BitwardenEu),
        (PasswordServer::BitwardenUs | PasswordServer::BitwardenEu, Some(_)) => Err(refuse(
            "is only for a self-hosted server; Bitwarden's own servers are fixed",
        )),
        (PasswordServer::SelfHosted, None) => Err(refuse("is required for a self-hosted server")),
        (PasswordServer::SelfHosted, Some(raw)) => {
            if raw.len() > MAX_SERVER_URL_LENGTH {
                return Err(refuse("is longer than 2048 characters"));
            }
            let url = Url::parse(raw).map_err(|_parse_error| refuse("is not a URL"))?;
            if url.scheme() != "https" {
                return Err(refuse("must be an https URL"));
            }
            if url.host_str().is_none() {
                return Err(refuse("must name a host"));
            }
            if !url.username().is_empty() || url.password().is_some() {
                return Err(refuse("must not carry credentials"));
            }
            if url.query().is_some() || url.fragment().is_some() {
                return Err(refuse("must not carry a query or a fragment"));
            }
            Ok(Server::SelfHosted(url))
        }
    }
}

/// A personal API key's client id, `user.<uuid>`, as Bitwarden shows it under Account
/// settings, Security, Keys. An organization's key cannot sign the CLI in.
fn validate_client_id(raw: &str) -> Result<String, ApiError> {
    let trimmed = raw.trim();
    if trimmed.len() > MAX_CLIENT_ID_LENGTH {
        return Err(ApiError::BadRequest(
            "clientId is longer than 200 characters".to_owned(),
        ));
    }
    if !trimmed.starts_with("user.") {
        return Err(ApiError::BadRequest(
            "clientId must be a personal API key's client_id, which starts with user.; an \
             organization's API key cannot sign the CLI in"
                .to_owned(),
        ));
    }
    Ok(trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitwarden_servers_are_fixed_and_self_hosted_ones_are_https() {
        assert_eq!(
            validate_server(PasswordServer::BitwardenEu, None).expect("fixed"),
            Server::BitwardenEu
        );
        validate_server(PasswordServer::BitwardenUs, Some("https://x.example.com"))
            .expect_err("refused");
        validate_server(PasswordServer::SelfHosted, None).expect_err("refused");
        validate_server(PasswordServer::SelfHosted, Some("http://vault.example.com"))
            .expect_err("refused");
        validate_server(
            PasswordServer::SelfHosted,
            Some("https://me:pw@vault.example.com"),
        )
        .expect_err("refused");
        validate_server(
            PasswordServer::SelfHosted,
            Some("https://vault.example.com/?x=1"),
        )
        .expect_err("refused");
        assert_eq!(
            validate_server(
                PasswordServer::SelfHosted,
                Some(" https://example.com/vault ")
            )
            .expect("a path is allowed")
            .origin()
            .as_str(),
            "https://example.com/vault"
        );
    }

    #[test]
    fn only_a_personal_api_key_signs_the_cli_in() {
        assert_eq!(
            validate_client_id(" user.6c2b0d8e-2f9e-4c52-9a8e-0e3c1f6e8d11 ").expect("personal"),
            "user.6c2b0d8e-2f9e-4c52-9a8e-0e3c1f6e8d11"
        );
        validate_client_id("organization.6c2b0d8e").expect_err("refused");
    }
}
