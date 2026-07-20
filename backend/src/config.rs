//! Deployment configuration derived from a small set of base overrides.
//!
//! A deployment is described by a hostname and a couple of ports; the values the
//! backend needs are derived from those, keeping `.env` short. The env var is
//! `ELYSIUM_HOSTNAME`, not `HOSTNAME`: Docker auto-sets `HOSTNAME` to the container
//! id and the host shell sets it to the machine name, so the bare name is unusable.

const DEFAULT_HOSTNAME: &str = "localhost";
const DEFAULT_FRONTEND_PORT: &str = "5173";

/// The env var naming the base directory Elysium may write local files to.
const DATA_DIR_VAR: &str = "ELYSIUM_DATA_DIR";

/// The env var naming the scoped Docker socket-proxy endpoint.
const DOCKER_PROXY_URL_VAR: &str = "ELYSIUM_DOCKER_PROXY_URL";

/// Where Elysium roots local files when `ELYSIUM_DATA_DIR` is unset.
///
/// A POSIX-absolute path, since Drives root on the deployment host's POSIX
/// filesystem (see `docs/storage.md`). In the Docker deployment this is the mounted
/// writable volume; that mount is the one deployment knob left, and it is a
/// compose-file detail, not app config. The single optional override is
/// `ELYSIUM_DATA_DIR`; everything else about storage is configured in-app.
const DEFAULT_DATA_DIR: &str = "/var/lib/elysium";

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

/// The base directory Elysium may write local files to.
///
/// Resolves `ELYSIUM_DATA_DIR` when set, otherwise [`DEFAULT_DATA_DIR`]. This is the
/// only storage-related deployment knob: auto-provisioned local-FS Drives root under
/// `<data_dir>/<org_id>/<project_id>`, and the resolved root is persisted in each
/// Drive's config, not re-derived at read time (see `crate::storage::defaults`).
pub fn data_dir() -> String {
    env_or(DATA_DIR_VAR, DEFAULT_DATA_DIR)
}

/// The scoped Docker socket-proxy endpoint, when the Docker backend is enabled.
///
/// Returns the `ELYSIUM_DOCKER_PROXY_URL` value (e.g. `http://socket-proxy:2375`)
/// or `None` when unset or blank. This names the engine transport, exactly like
/// `DATABASE_URL` or `REDIS_URL` name theirs, so it is boot/infra config and
/// belongs in the environment, not the in-app settings. When set, the
/// orchestration provisioner drives real containers through the proxy; when
/// unset, the in-memory backend keeps zero-config dev and test boot working.
pub fn docker_proxy_url() -> Option<String> {
    match std::env::var(DOCKER_PROXY_URL_VAR) {
        Ok(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}

/// Reads an environment variable, falling back to a default when unset.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_owned())
}
