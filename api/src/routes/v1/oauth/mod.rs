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
use url::Url;

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
    /// What the client calls itself; empty when it gave no name. Chosen by whoever registered
    /// it, so it proves nothing.
    name: String,
    /// The website it claims; just as self-chosen.
    uri: String,
    /// Where approving sends the authorization code: the host of each registered redirect URI,
    /// such as `localhost` for a CLI. The one thing about a self-registered client a person can
    /// check, since a code only ever goes there.
    redirect_hosts: Vec<String>,
    created_at: Option<DateTime<Utc>>,
}

impl From<Client> for ClientResponse {
    fn from(client: Client) -> Self {
        Self {
            id: client.client_id,
            name: client.client_name,
            uri: client.client_uri,
            redirect_hosts: redirect_hosts(&client.redirect_uris),
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

/// The distinct hosts of `uris`, in order. A URI that does not parse is shown whole, so nothing
/// a client registered is hidden.
fn redirect_hosts(uris: &[String]) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    for uri in uris {
        let host = Url::parse(uri)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| uri.clone());
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    hosts
}

/// What a client asked for that a person can grant, in the order Elysium lists scopes. Anything
/// else it asked for is left out; when nothing is left, the request is refused with
/// `invalid_scope` rather than granted a token that can do nothing.
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

    #[test]
    fn redirect_hosts_name_where_codes_go_once_each() {
        let uris = vec![
            "http://localhost:33418/callback".to_owned(),
            "http://localhost:33419/callback".to_owned(),
            "https://claude.ai/api/mcp/auth_callback".to_owned(),
            "not a url".to_owned(),
        ];
        assert_eq!(
            redirect_hosts(&uris),
            ["localhost", "claude.ai", "not a url"]
        );
    }
}
