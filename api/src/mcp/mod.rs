// Copyright © 2026 Jalapeno Labs

//! Elysium's MCP server, for external clients such as Claude Code and Codex, at `/api/mcp`.
//!
//! It speaks MCP's Streamable HTTP transport through the official Rust SDK, statelessly: every
//! request stands alone, so any API replica can answer it and nothing is kept between calls.
//! Every request carries an OAuth access token Hydra issued, checked by [`bearer`] before the
//! server sees it. The server is [`workspace`], and its tools are grouped by what they act on:
//! [`projects`], [`satellites`], and [`sessions`].
//!
//! Clients find out how to get a token from the protected resource metadata
//! ([`protected_resource_metadata`]), which names Hydra as the authorization server. See
//! `docs/mcp.md`.
//!
//! These are not the tools coding agents use on satellites (`crate::tools`), which are relayed
//! through the satellite and scoped to a session's project.

pub mod bearer;
mod projects;
mod satellites;
mod sessions;
pub mod workspace;

use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::middleware::from_fn_with_state;
use rmcp::transport::streamable_http_server::session::never::NeverSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use serde_json::{Value, json};

use self::workspace::Workspace;
use crate::oauth::{ADVERTISED_SCOPES, issuer, mcp_resource};
use crate::state::AppState;

/// The MCP server, mounted at `/api/mcp`, behind the bearer check.
pub fn router(state: &AppState) -> Router<AppState> {
    let public_url = &state.auth.public_url;
    let host = public_url
        .host_str()
        .expect("ELYSIUM_PUBLIC_URL was checked to have a host")
        .to_owned();
    // No sessions: each request is answered on its own, as JSON when it can be. nginx forwards
    // the host without its port, and only Elysium's own name is answered, which keeps DNS
    // rebinding from reaching the server through another one. A browser page on another site
    // may not call it; MCP clients send no Origin.
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_cancellation_token(state.shutdown.child_token())
        .with_allowed_hosts([host])
        .with_allowed_origins([public_url.origin().ascii_serialization()]);
    let workspace_state = state.clone();
    let service = StreamableHttpService::new(
        move || Ok(Workspace::new(workspace_state.clone())),
        Arc::new(NeverSessionManager::default()),
        config,
    );

    // Mounted at the nest's own path: a nested router's fallback answers only paths below it.
    Router::new()
        .route_service("/", service)
        .route_layer(from_fn_with_state(state.clone(), bearer::require_token))
}

/// `GET /.well-known/oauth-protected-resource[/api/mcp]`: the MCP server's protected resource
/// metadata (RFC 9728), which tells a client what the server is and where to get a token.
pub async fn protected_resource_metadata(State(state): State<AppState>) -> Json<Value> {
    let public_url = &state.auth.public_url;
    Json(json!({
        "resource": mcp_resource(public_url),
        "resource_name": "Elysium",
        "authorization_servers": [issuer(public_url)],
        "scopes_supported": ADVERTISED_SCOPES,
        "bearer_methods_supported": ["header"],
    }))
}
