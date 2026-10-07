// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/oauth/consent?challenge=`: what an MCP client asks the signed-in person for.
//!
//! Reading it changes nothing. A client the person already connected, asking for nothing more,
//! is answered `alreadyApproved`, and the page accepts it at once through
//! `POST /consent/accept`. `scopes` is empty when the client asked for nothing a person can
//! grant, and accepting it is then refused.

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{ClientResponse, grantable, own_consent};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
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

    Ok(Json(json!({
        "client": ClientResponse::from(request.client),
        "scopes": scopes,
        // Approved before for these scopes: the page answers at once, through the same POST as
        // pressing Allow, so the one change this feature makes stays behind the origin check.
        "alreadyApproved": request.skip,
    })))
}
