// Copyright © 2026 Jalapeno Labs

//! `/api/v1/password-vault`: the Bitwarden or Vaultwarden account agents sign in to things
//! with, and the one collection of it they may read. Admins alone manage it. See
//! `docs/passwords.md`.

mod choose_collection;
mod connect_password_vault;
mod delete_password_vault;
mod get_password_vault;
mod list_collections;
mod sync_password_vault;

use axum::Router;
use axum::routing::{get, post, put};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::password_vault::{PasswordServer, PasswordVault};
use crate::passwords::cli::CliError;
use crate::passwords::{VaultError, VaultStatus};
use crate::state::AppState;

/// The longest secret accepted. Bitwarden's API keys are 30 characters and its master
/// passwords far shorter than this; the bound only keeps a request from being enormous.
const MAX_SECRET_BYTES: usize = 1_024;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(get_password_vault::handle)
                .put(connect_password_vault::handle)
                .delete(delete_password_vault::handle),
        )
        .route("/collections", get(list_collections::handle))
        .route("/collection", put(choose_collection::handle))
        .route("/sync", post(sync_password_vault::handle))
}

/// The vault as clients see it. Its secrets never leave the API.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordVaultResponse {
    server: PasswordServer,
    server_url: Option<String>,
    client_id: String,
    account_email: String,
    collection: Option<CollectionResponse>,
    checked_at: DateTime<Utc>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResponse {
    id: Uuid,
    name: String,
    organization_id: Uuid,
}

impl From<&PasswordVault> for PasswordVaultResponse {
    fn from(vault: &PasswordVault) -> Self {
        Self {
            server: vault.server,
            server_url: vault.server_url.clone(),
            client_id: vault.client_id.clone(),
            account_email: vault.account_email.clone(),
            collection: vault.collection().map(|collection| CollectionResponse {
                id: collection.id,
                name: collection.name,
                organization_id: collection.organization_id,
            }),
            checked_at: vault.checked_at,
            created_by: vault.created_by,
            created_at: vault.created_at,
            updated_at: vault.updated_at,
        }
    }
}

/// The vault, if any, and what its CLI is doing, as every route answers it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultBody {
    vault: Option<PasswordVaultResponse>,
    status: VaultStatus,
}

/// A client secret or master password from a request body, checked as it is parsed so a
/// validator report never holds it, and moved straight into a [`SecretString`].
#[derive(Debug)]
pub struct VaultSecret(pub SecretString);

impl<'de> Deserialize<'de> for VaultSecret {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let secret = SecretString::from(String::deserialize(deserializer)?);
        if secret.expose_secret().is_empty() {
            return Err(serde::de::Error::custom("must not be empty"));
        }
        if secret.expose_secret().len() > MAX_SECRET_BYTES {
            return Err(serde::de::Error::custom("exceeds 1 KiB"));
        }
        Ok(Self(secret))
    }
}

/// Maps a vault failure to the answer an admin reads: the CLI's own refusal as a bad request,
/// since it names what to fix, and the CLI failing to run as a bad gateway.
fn vault_error(error: VaultError) -> ApiError {
    match error {
        VaultError::Cli(CliError::Refused(message)) => {
            ApiError::BadRequest(format!("Bitwarden refused: {message}"))
        }
        VaultError::Cli(CliError::Failed(message)) => ApiError::BadGateway(message),
        VaultError::NotConfigured => ApiError::NotFound,
        VaultError::NotReady => {
            ApiError::Unavailable("the password vault is not ready; its status says why")
        }
        VaultError::NoCollection | VaultError::NotInCollection(_) => {
            ApiError::BadRequest(error.to_string())
        }
        VaultError::Internal(error) => ApiError::Internal(error),
    }
}
