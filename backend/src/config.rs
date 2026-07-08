//! Deployment configuration derived from a small set of base overrides.
//!
//! Rather than spelling out every URL in the environment, a deployment is
//! described by a hostname, a few ports, and the realm; every Keycloak, OIDC, and
//! CORS URL is derived from those. This keeps `.env` short and consistent: change
//! the host or a port in one place and the whole system follows.
//!
//! Note the env var is `ELYSIUM_HOSTNAME`, not `HOSTNAME`: Docker auto-sets
//! `HOSTNAME` to the container id and the host shell sets it to the machine name,
//! so the bare name is unusable here.

/// Compose service and container port Keycloak is reachable at inside the network.
const DEFAULT_INTERNAL_KEYCLOAK: &str = "keycloak:8080";

const DEFAULT_HOSTNAME: &str = "localhost";
const DEFAULT_API_PORT: &str = "31416";
const DEFAULT_KEYCLOAK_PORT: &str = "8080";
const DEFAULT_FRONTEND_PORT: &str = "5173";

/// Realm name, fixed to match `keycloak/realm-export.json`. Not configurable:
/// changing it means re-authoring the realm, so there is nothing to override.
const REALM: &str = "elysium";

/// Every externally-shaped URL the backend needs, derived from the base config.
///
/// Browser-facing URLs (`issuer`, `authorize_url`, `account_url`, `redirect_uri`,
/// `app_origin`) use the public hostname so redirects land on Elysium's origin;
/// the backchannel URLs (`jwks_url`, `token_url`, `logout_url`) use the internal
/// Keycloak host reachable from inside the Docker network.
#[derive(Debug, Clone)]
pub struct Deployment {
    pub issuer: String,
    pub jwks_url: String,
    pub authorize_url: String,
    pub token_url: String,
    pub logout_url: String,
    pub account_url: String,
    pub redirect_uri: String,
    pub app_origin: String,
}

impl Deployment {
    /// Derives all URLs from the base environment overrides, each with a default.
    pub fn from_env() -> Self {
        let host = env_or("ELYSIUM_HOSTNAME", DEFAULT_HOSTNAME);
        let api_port = env_or("API_PORT", DEFAULT_API_PORT);
        let keycloak_port = env_or("KEYCLOAK_PORT", DEFAULT_KEYCLOAK_PORT);
        let frontend_port = env_or("FRONTEND_PORT", DEFAULT_FRONTEND_PORT);
        let internal = env_or("KEYCLOAK_INTERNAL_HOST", DEFAULT_INTERNAL_KEYCLOAK);

        let browser_realm = format!("http://{host}:{keycloak_port}/realms/{REALM}");
        let internal_realm = format!("http://{internal}/realms/{REALM}");

        Self {
            authorize_url: format!("{browser_realm}/protocol/openid-connect/auth"),
            account_url: format!("{browser_realm}/account"),
            token_url: format!("{internal_realm}/protocol/openid-connect/token"),
            logout_url: format!("{internal_realm}/protocol/openid-connect/logout"),
            jwks_url: format!("{internal_realm}/protocol/openid-connect/certs"),
            redirect_uri: format!("http://{host}:{api_port}/auth/callback"),
            app_origin: format!("http://{host}:{frontend_port}"),
            issuer: browser_realm,
        }
    }
}

/// Reads an environment variable, falling back to a default when unset.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}
