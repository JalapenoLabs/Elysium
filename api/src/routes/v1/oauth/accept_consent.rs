// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/oauth/consent/accept`: connect an MCP client. It is granted what it asked for that
//! a person can grant, for Elysium's MCP server alone. Answers where the browser goes next.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde_json::{Value, json};
use tracing::{Level, event};

use super::{ChallengeBody, grantable, own_consent};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::oauth::mcp_resource;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<ChallengeBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let request = own_consent(&state, &current, &body.challenge).await?;
    let scopes = grantable(&request.requested_scope);
    // Every token is bound to the MCP server, whatever the client asked: Hydra ignores the
    // `resource` parameter MCP clients send, so the audience is set here. See docs/mcp.md.
    let audience = mcp_resource(&state.auth.public_url);

    let hydra = &state.auth.hydra;
    hydra.allow_audience(&request.client, &audience).await?;
    let redirect_to = hydra
        .accept_consent(&body.challenge, &scopes, &audience)
        .await?;
    event!(
        name: "oauth.consent.granted",
        Level::INFO,
        user.id = %current.id(),
        oauth.client.id = request.client.client_id,
        oauth.scopes = scopes.join(" "),
        "connected an MCP client",
    );
    Ok(Json(json!({ "redirectTo": redirect_to })))
}
