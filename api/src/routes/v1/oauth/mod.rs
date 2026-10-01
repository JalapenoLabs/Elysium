// Copyright © 2026 Jalapeno Labs

//! `/api/v1/oauth`: the signed-in person's side of OAuth for MCP clients. See `docs/mcp.md`.
//!
//! Hydra sends a person connecting an MCP client to the web app's `/oauth/login` and
//! `/oauth/consent` pages with a challenge; those pages answer it here. Everything here needs an
//! active, approved person, like every workspace route, so OAuth reaches no one admins have not
//! let in. People also list and revoke the clients they connected, and admins see and delete
//! every registered client.

mod accept_consent;
mod accept_login;
mod delete_client;
mod get_consent;
mod list_clients;
mod list_grants;
mod reject_consent;
mod revoke_grant;

use axum::Router;
use axum::routing::{delete, get, post};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::oauth::GRANTABLE_SCOPES;
use crate::oauth::hydra::{Client, ConsentRequest};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login/accept", post(accept_login::handle))
        .route("/consent", get(get_consent::handle))
        .route("/consent/accept", post(accept_consent::handle))
        .route("/consent/reject", post(reject_consent::handle))
        .route("/grants", get(list_grants::handle))
        .route("/grants/{client_id}", delete(revoke_grant::handle))
        .route("/clients", get(list_clients::handle))
        .route("/clients/{client_id}", delete(delete_client::handle))
}

/// A challenge Hydra handed the web app, answered in a request body so it never sits in a log
/// line's URL.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChallengeBody {
    challenge: String,
}

/// An OAuth client as people see it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientResponse {
    id: String,
    /// What the client calls itself; empty when it gave no name.
    name: String,
    uri: String,
    logo_uri: String,
    created_at: Option<DateTime<Utc>>,
}

impl From<Client> for ClientResponse {
    fn from(client: Client) -> Self {
        Self {
            id: client.client_id,
            name: client.client_name,
            uri: client.client_uri,
            logo_uri: client.logo_uri,
            created_at: client.created_at,
        }
    }
}

/// The consent request behind `challenge`, refused unless the person answering is the one who
/// signed in for it: a challenge is no use to anyone else.
async fn own_consent(
    state: &AppState,
    current: &CurrentUser,
    challenge: &str,
) -> Result<ConsentRequest, ApiError> {
    let request = state.auth.hydra.consent_request(challenge).await?;
    if request.subject != current.id().to_string() {
        return Err(ApiError::Forbidden(
            "this request to connect an application belongs to someone else".to_owned(),
        ));
    }
    Ok(request)
}

/// What a client asked for that a person can grant, in the order Elysium lists scopes. Anything
/// else it asked for is left out, rather than refusing the whole request.
fn grantable(requested: &[String]) -> Vec<String> {
    GRANTABLE_SCOPES
        .iter()
        .filter(|scope| requested.iter().any(|asked| asked == *scope))
        .map(|scope| (*scope).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_only_known_scopes_in_a_stable_order() {
        let requested = vec![
            "openid".to_owned(),
            "workspace:write".to_owned(),
            "workspace:read".to_owned(),
            "workspace:admin".to_owned(),
            "offline_access".to_owned(),
        ];
        assert_eq!(
            grantable(&requested),
            ["workspace:read", "workspace:write", "offline_access"]
        );
        assert!(grantable(&[]).is_empty());
    }
}
