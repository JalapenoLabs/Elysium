// Copyright © 2026 Jalapeno Labs

//! OAuth for MCP clients, through Ory Hydra.
//!
//! An MCP client such as Claude Code or Codex reaches Elysium's MCP server at `/api/mcp` with an
//! access token Hydra issued. Getting one is OAuth 2.1 with PKCE: the client registers itself
//! with Hydra, sends the person to Hydra, and Hydra hands the sign-in and the consent to
//! Elysium's `/oauth/login` and `/oauth/consent` pages, which answer them through the routes
//! under `/api/v1/oauth` as whoever Kratos says is signed in. Only an approved, active person can
//! answer, so OAuth reaches no one admins have not let in. See `docs/mcp.md`.
//!
//! [`hydra`] is the admin API client. The scopes Elysium grants are below; the MCP server checks
//! them on every call (`crate::mcp`).

pub mod hydra;

use url::Url;

/// Reading the workspace. Every MCP call needs it.
pub const SCOPE_READ: &str = "workspace:read";

/// Changing the workspace. Tools that write need it as well.
pub const SCOPE_WRITE: &str = "workspace:write";

/// Asking for refresh tokens. Hydra issues one only when it was granted.
pub const SCOPE_OFFLINE: &str = "offline_access";

/// Every scope a person can grant a client. A client asking for anything else is granted the
/// part of it that is here. Must match `hydra/hydra.yml`.
pub const GRANTABLE_SCOPES: [&str; 3] = [SCOPE_READ, SCOPE_WRITE, SCOPE_OFFLINE];

/// The scopes the MCP server's metadata advertises: what basic use needs. Refresh tokens are the
/// client's to ask for, not the server's to require (the MCP authorization spec).
pub const ADVERTISED_SCOPES: [&str; 2] = [SCOPE_READ, SCOPE_WRITE];

/// The MCP server's canonical URI: the audience every token for it is bound to, and what its
/// metadata names as the resource.
pub fn mcp_resource(public_url: &Url) -> String {
    format!("{}/api/mcp", public_url.origin().ascii_serialization())
}

/// Where the MCP server's protected resource metadata lives, per RFC 9728: the well-known prefix
/// inserted before the resource's path.
pub fn mcp_resource_metadata_url(public_url: &Url) -> String {
    format!(
        "{}/.well-known/oauth-protected-resource/api/mcp",
        public_url.origin().ascii_serialization()
    )
}

/// The issuer MCP clients are sent to: Hydra, on Elysium's own origin.
pub fn issuer(public_url: &Url) -> String {
    public_url.origin().ascii_serialization()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_hang_off_the_origin_without_a_trailing_slash() {
        let public = Url::parse("https://work.example.com/").expect("a URL");
        assert_eq!(mcp_resource(&public), "https://work.example.com/api/mcp");
        assert_eq!(
            mcp_resource_metadata_url(&public),
            "https://work.example.com/.well-known/oauth-protected-resource/api/mcp"
        );
        assert_eq!(issuer(&public), "https://work.example.com");

        let local = Url::parse("http://localhost:8099").expect("a URL");
        assert_eq!(mcp_resource(&local), "http://localhost:8099/api/mcp");
    }
}
