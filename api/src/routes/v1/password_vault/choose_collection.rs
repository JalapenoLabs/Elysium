// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/password-vault/collection`: choose the one collection agents may read, or clear
//! it with `null` so they read nothing.
//!
//! The id must be one the account reaches now, as the CLI answers; its name and organization
//! are stored as the CLI reports them.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use uuid::Uuid;

use super::{PasswordVaultResponse, VaultBody, vault_error};
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::password_vault::{self, ChosenCollection};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(deserialize_with = "Option::deserialize")]
    collection_id: Option<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<VaultBody>, ApiError> {
    let Json(body) = body?;
    let chosen = match body.collection_id {
        None => None,
        Some(id) => {
            let collections = state.passwords.collections().await.map_err(vault_error)?;
            let collection = collections
                .into_iter()
                .find(|collection| collection.id == id)
                .ok_or_else(|| {
                    ApiError::BadRequest(format!(
                        "collection {id} is not one the vault's account reaches"
                    ))
                })?;
            Some(ChosenCollection {
                id: collection.id,
                name: collection.name,
                organization_id: collection.organization_id,
            })
        }
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let vault = password_vault::choose_collection(&mut connection, chosen.as_ref()).await?;
    let response = PasswordVaultResponse::from(&vault);
    state
        .events
        .publish(&ServerEvent::PasswordVaultUpdated(response.clone()));
    Ok(Json(VaultBody {
        vault: Some(response),
        status: state.passwords.status(),
    }))
}
