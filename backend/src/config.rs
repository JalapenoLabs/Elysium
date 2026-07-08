//! Deployment configuration derived from a small set of base overrides.
//!
//! A deployment is described by a hostname and a couple of ports; the values the
//! backend needs are derived from those, keeping `.env` short. The env var is
//! `ELYSIUM_HOSTNAME`, not `HOSTNAME`: Docker auto-sets `HOSTNAME` to the container
//! id and the host shell sets it to the machine name, so the bare name is unusable.

const DEFAULT_HOSTNAME: &str = "localhost";
const DEFAULT_FRONTEND_PORT: &str = "5173";

/// The externally-shaped values the backend derives from the base config.
///
/// The SPA and API share one origin (the SPA proxies `/api` to the backend), so
/// `app_origin` is both the session-cookie origin and the WebAuthn RP origin, and
/// `rp_id` is that origin's registrable domain.
#[derive(Debug, Clone)]
pub struct Deployment {
    /// SPA origin, e.g. `http://localhost:5173`. The WebAuthn RP origin.
    pub app_origin: String,
    /// WebAuthn Relying Party ID: the registrable domain (host, no scheme/port).
    pub rp_id: String,
}

impl Deployment {
    /// Derives the deployment values from the base environment overrides.
    pub fn from_env() -> Self {
        let host = env_or("ELYSIUM_HOSTNAME", DEFAULT_HOSTNAME);
        let frontend_port = env_or("FRONTEND_PORT", DEFAULT_FRONTEND_PORT);

        Self {
            app_origin: format!("http://{host}:{frontend_port}"),
            rp_id: host,
        }
    }
}

/// Reads an environment variable, falling back to a default when unset.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}
