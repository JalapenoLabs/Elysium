// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/oauth/clients`: every registered MCP client, for admins. Clients register
//! themselves, so this is how an admin sees what has.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::ClientResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    _admin: AdminUser,
) -> Result<Json<Value>, ApiError> {
    let clients: Vec<ClientResponse> = state
        .auth
        .hydra
        .clients()
        .await?
        .into_iter()
        .map(ClientResponse::from)
        .collect();
    Ok(Json(json!({ "clients": clients })))
}
