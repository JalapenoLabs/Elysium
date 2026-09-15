// Copyright © 2026 Jalapeno Labs

//! Process configuration, read once from the environment at startup.
//!
//! Every knob has a documented default except the connection URLs and the
//! encryption key, which are required so a misconfigured deployment fails before it
//! binds a port. Values that grant access are held as [`SecretString`] so a stray
//! `{:?}` cannot print them.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
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
    pub database_max_connections: u32,
    /// Origins allowed by CORS. Empty means no cross-origin access, which is the
    /// normal state behind nginx where the frontend shares the API's origin.
    pub cors_allowed_origins: Vec<HeaderValue>,
    pub request_timeout: Duration,
    pub max_request_body_bytes: usize,
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
}

impl Config {
    /// Builds the configuration from environment variables.
    ///
    /// # Errors
    /// Returns an error when `DATABASE_URL`, `REDIS_URL`, or `ELYSIUM_ENCRYPTION_KEY`
    /// is missing, or when any numeric variable does not parse.
    pub fn from_env() -> Result<Self> {
        let host: IpAddr = env_or("HOST", IpAddr::V4(Ipv4Addr::UNSPECIFIED))?;
        let port: u16 = env_or("PORT", 8080)?;

        Ok(Self {
            bind_address: SocketAddr::new(host, port),
            database_url: required_secret("DATABASE_URL")?,
            redis_url: required_secret("REDIS_URL")?,
            encryption_key: required_secret("ELYSIUM_ENCRYPTION_KEY")?,
            database_max_connections: env_or("DATABASE_MAX_CONNECTIONS", 10)?,
            cors_allowed_origins: parse_cors_origins()?,
            request_timeout: Duration::from_secs(env_or("REQUEST_TIMEOUT_SECONDS", 30)?),
            max_request_body_bytes: env_or("MAX_REQUEST_BODY_BYTES", 1_048_576)?,
            mail: MailConfig {
                oauth_broker: optional_base_url("OAUTH_BROKER_URL")?,
                oauth_broker_internal: optional_base_url("OAUTH_BROKER_INTERNAL_URL")?,
                docker: optional_base_url("DOCKER_URL")?.unwrap_or_else(|| {
                    Url::parse("http://docker-proxy:2375/").expect("a static URL parses")
                }),
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
