// Copyright © 2026 Jalapeno Labs

//! Broker configuration, read once from the environment at startup.
//!
//! The broker is its own deployable, so its OAuth app secrets are its bootstrap
//! configuration: they come from the environment of wherever it is hosted. None of
//! them belong in an Elysium instance's `.env`.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use anyhow::{Context, Result, bail};
use secrecy::SecretString;
use url::Url;

use crate::providers::{AppCredentials, Provider};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_address: SocketAddr,
    /// Where browsers reach the broker. Providers redirect to
    /// `<public_url>/v1/callback/<provider>`, which must match the redirect URI
    /// registered with each OAuth app exactly.
    pub public_url: Url,
    /// Base64 32-byte key sealing state and handoff tokens. Every replica shares it.
    pub sealing_key: SecretString,
    pub google: Option<AppCredentials>,
    pub microsoft: Option<AppCredentials>,
}

impl Config {
    /// Builds the configuration from environment variables.
    ///
    /// # Errors
    /// Fails when `BROKER_PUBLIC_URL` or `BROKER_SEALING_KEY` is missing or invalid,
    /// when a provider has only one of its two credentials, or when no provider is
    /// configured at all.
    pub fn from_env() -> Result<Self> {
        let port: u16 = match std::env::var("PORT") {
            Ok(raw) => raw
                .trim()
                .parse()
                .with_context(|| format!("PORT is invalid: {raw:?}"))?,
            Err(_unset) => 8080,
        };

        let raw_public_url = required("BROKER_PUBLIC_URL")?;
        let public_url = Url::parse(raw_public_url.trim_end_matches('/'))
            .with_context(|| format!("BROKER_PUBLIC_URL is not a URL: {raw_public_url:?}"))?;

        let config = Self {
            bind_address: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
            public_url,
            sealing_key: SecretString::from(required("BROKER_SEALING_KEY")?),
            google: app_credentials("GOOGLE")?,
            microsoft: app_credentials("MICROSOFT")?,
        };

        if config.google.is_none() && config.microsoft.is_none() {
            bail!(
                "no provider is configured: set GOOGLE_CLIENT_ID/SECRET or MICROSOFT_CLIENT_ID/SECRET"
            );
        }
        Ok(config)
    }

    /// The OAuth app for `provider`, or `None` when this deployment does not offer it.
    pub const fn credentials(&self, provider: Provider) -> Option<&AppCredentials> {
        match provider {
            Provider::Google => self.google.as_ref(),
            Provider::Microsoft => self.microsoft.as_ref(),
        }
    }

    /// The redirect URI registered with `provider`'s OAuth app.
    pub fn callback_url(&self, provider: Provider) -> String {
        format!(
            "{}/v1/callback/{}",
            self.public_url.as_str().trim_end_matches('/'),
            provider.spec().slug,
        )
    }
}

fn required(key: &str) -> Result<String> {
    let value = std::env::var(key).with_context(|| format!("{key} is required"))?;
    if value.trim().is_empty() {
        bail!("{key} is set but empty");
    }
    Ok(value)
}

/// Reads `<PREFIX>_CLIENT_ID` and `<PREFIX>_CLIENT_SECRET`. Both or neither.
fn app_credentials(prefix: &str) -> Result<Option<AppCredentials>> {
    let client_id = std::env::var(format!("{prefix}_CLIENT_ID")).unwrap_or_default();
    let client_secret = std::env::var(format!("{prefix}_CLIENT_SECRET")).unwrap_or_default();

    match (client_id.trim().is_empty(), client_secret.trim().is_empty()) {
        (true, true) => Ok(None),
        (false, false) => Ok(Some(AppCredentials {
            client_id: client_id.trim().to_owned(),
            client_secret: SecretString::from(client_secret.trim().to_owned()),
        })),
        _ => bail!("{prefix}_CLIENT_ID and {prefix}_CLIENT_SECRET must be set together"),
    }
}
