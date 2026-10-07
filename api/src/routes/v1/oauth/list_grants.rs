// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/oauth/grants`: the MCP clients the signed-in person connected, newest first, one
//! entry per client with every scope it holds and when it was last granted.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::State;
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};

use super::ClientResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::oauth::hydra::ConsentSession;
use crate::state::AppState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct GrantResponse {
    client: ClientResponse,
    scopes: Vec<String>,
    granted_at: Option<DateTime<Utc>>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
) -> Result<Json<Value>, ApiError> {
    let sessions = state.auth.hydra.consent_sessions(current.id()).await?;
    Ok(Json(json!({ "grants": by_client(sessions) })))
}

/// One grant per client. Hydra keeps a consent session per time a client was connected; a person
/// sees one connection holding the union of their scopes.
fn by_client(sessions: Vec<ConsentSession>) -> Vec<GrantResponse> {
    let mut grants: BTreeMap<String, GrantResponse> = BTreeMap::new();
    for session in sessions {
        let client = session.consent_request.client;
        let grant = grants
            .entry(client.client_id.clone())
            .or_insert_with(|| GrantResponse {
                client: ClientResponse::from(client),
                scopes: Vec::new(),
                granted_at: None,
            });
        for scope in session.grant_scope {
            if !grant.scopes.contains(&scope) {
                grant.scopes.push(scope);
            }
        }
        grant.granted_at = grant.granted_at.max(session.handled_at);
    }
    // Newest first, so the client just connected leads the list.
    let mut grants: Vec<GrantResponse> = grants.into_values().collect();
    grants.sort_by_key(|grant| std::cmp::Reverse(grant.granted_at));
    grants
}
