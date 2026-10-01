// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/oauth/consent?challenge=`: what an MCP client asks the signed-in person for.
//!
//! A client the person already connected, asking for nothing more, is connected again without
//! asking: the answer then carries `redirectTo` alone.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ClientResponse, grantable, own_consent};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::oauth::mcp_resource;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct ConsentQuery {
    challenge: String,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<ConsentQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let request = own_consent(&state, &current, &query.challenge).await?;
    let scopes = grantable(&request.requested_scope);

    if request.skip {
        let audience = mcp_resource(&state.auth.public_url);
        let hydra = &state.auth.hydra;
        hydra.allow_audience(&request.client, &audience).await?;
        let redirect_to = hydra
            .accept_consent(&query.challenge, &scopes, &audience)
            .await?;
        return Ok(Json(json!({ "redirectTo": redirect_to })));
    }

    Ok(Json(json!({
        "client": ClientResponse::from(request.client),
        "scopes": scopes,
    })))
}
