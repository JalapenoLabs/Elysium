// Copyright © 2026 Jalapeno Labs

//! Process configuration, read once from the environment at startup.
//!
//! Every knob has a documented default except the connection URLs and the
//! encryption key, which are required so a misconfigured deployment fails before it
//! binds a port. Values that grant access are held as [`SecretString`] so a stray
//! `{:?}` cannot print them.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::http::HeaderValue;
use secrecy::SecretString;
use url::Url;

/// Fully resolved runtime configuration.
#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: SocketAddr,
    pub database_url: SecretString,
    pub redis_url: SecretString,
    /// Base64 key sealing secrets stored in Postgres. See `crate::crypto`.
    pub encryption_key: SecretString,
    /// The origin browsers reach Elysium at: `https://<host>`, or `http://localhost` for
    /// development. Sessions, passkeys, and recovery links are bound to it.
    pub public_url: Url,
    /// Kratos's public API, where sessions are checked.
    pub kratos_public_url: Url,
    /// Kratos's admin API, where identities and sessions are managed.
    pub kratos_admin_url: Url,
    /// Hydra's admin API: sign-in and consent for MCP clients, and token introspection.
    pub hydra_admin_url: Url,
    pub database_max_connections: u32,
    /// Origins allowed by CORS. Empty means no cross-origin access, which is the
    /// normal state behind nginx where the frontend shares the API's origin.
    pub cors_allowed_origins: Vec<HeaderValue>,
    pub request_timeout: Duration,
    pub max_request_body_bytes: usize,
    /// The web app's production build, served at every path outside `/api`. Set by the
    /// published image; unset in development, where nginx sends those paths to Vite.
    pub frontend_dir: Option<PathBuf>,
    pub mail: MailConfig,
}

/// Where the mail services are. Without a broker, Gmail and Outlook are unavailable and
/// the settings page says so.
#[derive(Debug, Clone)]
pub struct MailConfig {
    /// The OAuth broker as browsers reach it. Unset disables Gmail and Outlook.
    pub oauth_broker: Option<Url>,
    /// The broker as the API reaches it, when that differs from what browsers see.
    pub oauth_broker_internal: Option<Url>,
    /// The Docker API the mail server is run through: the filtered socket proxy.
    pub docker: Url,
    /// nginx's fixed address on the mail network. Stalwart trusts the PROXY protocol
    /// header, which carries each client's real address, from it alone. Unset when
    /// mail does not arrive through nginx.
    pub ingress_address: Option<IpAddr>,
}

impl Config {
    /// Builds the configuration from environment variables.
    ///
    /// # Errors
    /// Returns an error when `DATABASE_URL`, `REDIS_URL`, `ELYSIUM_ENCRYPTION_KEY`,
    /// `ELYSIUM_PUBLIC_URL`, `KRATOS_PUBLIC_URL`, `KRATOS_ADMIN_URL`, or `HYDRA_ADMIN_URL` is
    /// missing, when the public URL is not one browsers can hold sessions for, or when any
    /// numeric variable does not parse.
    pub fn from_env() -> Result<Self> {
        let host: IpAddr = env_or("HOST", IpAddr::V4(Ipv4Addr::UNSPECIFIED))?;
        let port: u16 = env_or("PORT", 8080)?;

        Ok(Self {
            bind_address: SocketAddr::new(host, port),
            database_url: required_secret("DATABASE_URL")?,
            redis_url: required_secret("REDIS_URL")?,
            encryption_key: required_secret("ELYSIUM_ENCRYPTION_KEY")?,
            public_url: parse_public_url(&required("ELYSIUM_PUBLIC_URL")?)?,
            kratos_public_url: required_base_url("KRATOS_PUBLIC_URL")?,
            kratos_admin_url: required_base_url("KRATOS_ADMIN_URL")?,
            hydra_admin_url: required_base_url("HYDRA_ADMIN_URL")?,
            database_max_connections: env_or("DATABASE_MAX_CONNECTIONS", 10)?,
            cors_allowed_origins: parse_cors_origins()?,
            request_timeout: Duration::from_secs(env_or("REQUEST_TIMEOUT_SECONDS", 30)?),
            max_request_body_bytes: env_or("MAX_REQUEST_BODY_BYTES", 1_048_576)?,
            frontend_dir: std::env::var_os("FRONTEND_DIR")
                .filter(|directory| !directory.is_empty())
                .map(PathBuf::from),
            mail: MailConfig {
                oauth_broker: optional_base_url("OAUTH_BROKER_URL")?,
                oauth_broker_internal: optional_base_url("OAUTH_BROKER_INTERNAL_URL")?,
                docker: optional_base_url("DOCKER_URL")?.unwrap_or_else(|| {
                    Url::parse("http://docker-proxy:2375/").expect("a static URL parses")
                }),
                ingress_address: std::env::var("MAIL_INGRESS_ADDRESS")
                    .ok()
                    .filter(|address| !address.trim().is_empty())
                    .map(|address| address.trim().parse())
                    .transpose()
                    .context("MAIL_INGRESS_ADDRESS is not an IP address")?,
            },
        })
    }
}

/// Reads a variable that must be present, wrapping it so it cannot be logged.
///
/// # Errors
/// Fails when the variable is unset or empty.
pub fn required_secret(key: &str) -> Result<SecretString> {
    let value = std::env::var(key).with_context(|| format!("{key} is required"))?;
    if value.trim().is_empty() {
        anyhow::bail!("{key} is set but empty");
    }
    Ok(SecretString::from(value))
}

/// Reads a variable that must be present and non-empty.
fn required(key: &str) -> Result<String> {
    let value = std::env::var(key).with_context(|| format!("{key} is required"))?;
    if value.trim().is_empty() {
        anyhow::bail!("{key} is set but empty");
    }
    Ok(value.trim().to_owned())
}

/// Reads a base URL that must be present.
fn required_base_url(key: &str) -> Result<Url> {
    optional_base_url(key)?.with_context(|| format!("{key} is required"))
}

/// Checks `ELYSIUM_PUBLIC_URL`: a bare origin, over https, or over http on `localhost`
/// alone. Browsers hold passkeys and `Secure` session cookies only for such an origin, and
/// `WebAuthn` refuses an IP address, so anything else would fail at sign-in instead of here.
fn parse_public_url(raw: &str) -> Result<Url> {
    let url =
        Url::parse(raw).with_context(|| format!("ELYSIUM_PUBLIC_URL is not a URL: {raw:?}"))?;
    let host = url.host_str().unwrap_or_default();
    let secure = match url.scheme() {
        "https" => url.domain().is_some(),
        "http" => host == "localhost",
        _ => false,
    };
    if !secure {
        anyhow::bail!(
            "ELYSIUM_PUBLIC_URL must be https://<host name>, or http://localhost for development: {raw:?}"
        );
    }
    if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        anyhow::bail!("ELYSIUM_PUBLIC_URL must be an origin, with no path: {raw:?}");
    }
    Ok(url)
}

/// Reads `key` from the environment, falling back to `default` when it is unset.
fn env_or<Value>(key: &str, default: Value) -> Result<Value>
where
    Value: FromStr,
    Value::Err: std::error::Error + Send + Sync + 'static,
{
    let Ok(raw) = std::env::var(key) else {
        return Ok(default);
    };

    raw.trim()
        .parse()
        .with_context(|| format!("{key} has an invalid value: {raw:?}"))
}

/// Reads an optional base URL. Empty counts as unset. The path gains a trailing slash
/// so that joining a relative path appends to it rather than replacing its last segment.
fn optional_base_url(key: &str) -> Result<Option<Url>> {
    let raw = std::env::var(key).unwrap_or_default();
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let mut url = Url::parse(trimmed).with_context(|| format!("{key} is not a URL: {raw:?}"))?;
    if !url.path().ends_with('/') {
        url.set_path(&format!("{}/", url.path()));
    }
    Ok(Some(url))
}

/// Parses the comma-separated `CORS_ALLOWED_ORIGINS` list into header values.
fn parse_cors_origins() -> Result<Vec<HeaderValue>> {
    let Ok(raw) = std::env::var("CORS_ALLOWED_ORIGINS") else {
        return Ok(Vec::new());
    };

    raw.split(',')
        .map(str::trim)
        .filter(|origin| !origin.is_empty())
        .map(|origin| {
            HeaderValue::from_str(origin).with_context(|| {
                format!("CORS_ALLOWED_ORIGINS entry is not a valid header value: {origin:?}")
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_urls_are_https_hosts_or_localhost() {
        for accepted in [
            "https://elysium.example.com",
            "https://elysium.example.com/",
            "https://elysium.example.com:8443",
            "http://localhost:4000",
        ] {
            parse_public_url(accepted).unwrap_or_else(|error| panic!("{accepted}: {error}"));
        }
    }

    #[test]
    fn public_urls_refuse_what_browsers_cannot_hold_sessions_for() {
        for refused in [
            "http://192.168.2.101:4000",
            "http://elysium.example.com",
            "https://192.168.2.101",
            "https://elysium.example.com/app",
            "https://elysium.example.com/?x=1",
            "ftp://localhost",
            "localhost:4000",
        ] {
            assert!(parse_public_url(refused).is_err(), "{refused} is refused");
        }
    }
}
