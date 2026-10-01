// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/oauth/clients/{client_id}`: remove a registered MCP client, for admins. Every
//! grant and token it holds, for everyone, stops working; it can register again.

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use tracing::{Level, event};

use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(client_id) = path?;
    state.auth.hydra.delete_client(&client_id).await?;
    event!(
        name: "oauth.client.deleted",
        Level::INFO,
        user.id = %admin.id(),
        oauth.client.id = client_id,
        "an admin deleted an MCP client",
    );
    Ok(StatusCode::NO_CONTENT)
}
