// Copyright © 2026 Jalapeno Labs

//! `/api/v1/satellites`: Arsox satellites. Secrets are write-only over HTTP.

mod create_satellite;
mod delete_satellite;
mod get_satellite;
mod list_satellites;
mod test_satellite;
mod update_satellite;

use axum::Router;
use axum::routing::{get, post};
use chrono::{DateTime, Utc};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;
use validator::ValidationError;

use crate::fleet::views::SatelliteStatus;
use crate::models::satellite::Satellite;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_satellites::handle).post(create_satellite::handle),
        )
        .route(
            "/{id}",
            get(get_satellite::handle)
                .patch(update_satellite::handle)
                .delete(delete_satellite::handle),
        )
        .route("/{id}/test", post(test_satellite::handle))
}

/// Upper bound on a stored bearer secret. Arsox secrets are 64 hex characters; this
/// leaves room for other generators without letting one row carry an arbitrary blob.
const SECRET_MAX_BYTES: usize = 1024;

/// A satellite as clients see it. The secret is never included, sealed or not.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SatelliteResponse {
    id: Uuid,
    name: String,
    description: String,
    url: String,
    is_active: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    /// The fleet's latest poll; `None` for an inactive satellite or before the first poll.
    status: Option<SatelliteStatus>,
}

impl SatelliteResponse {
    pub fn new(satellite: Satellite, status: Option<SatelliteStatus>) -> Self {
        Self {
            id: satellite.id,
            name: satellite.name,
            description: satellite.description,
            url: satellite.url,
            is_active: satellite.is_active,
            created_at: satellite.created_at,
            updated_at: satellite.updated_at,
            status,
        }
    }
}

/// Rejects names that are only whitespace; `length` alone would accept `"   "`.
fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// Satellites are reached over HTTP, so any other scheme is a mistake.
fn validate_http_scheme(value: &str) -> Result<(), ValidationError> {
    if value.starts_with("http://") || value.starts_with("https://") {
        return Ok(());
    }
    Err(ValidationError::new("scheme").with_message("must start with http:// or https://".into()))
}

/// A bearer secret from a request body, checked as it is parsed.
///
/// Validation happens during deserialization rather than through `validator`,
/// because a validator error report would need to serialize the value itself.
#[derive(Debug)]
pub struct SatelliteSecret(pub SecretString);

impl<'de> Deserialize<'de> for SatelliteSecret {
    fn deserialize<Source: Deserializer<'de>>(deserializer: Source) -> Result<Self, Source::Error> {
        let secret = SecretString::from(String::deserialize(deserializer)?);

        if secret.expose_secret().trim().is_empty() {
            return Err(serde::de::Error::custom("secret must not be blank"));
        }
        if secret.expose_secret().len() > SECRET_MAX_BYTES {
            return Err(serde::de::Error::custom("secret exceeds 1 KiB"));
        }

        Ok(Self(secret))
    }
}
