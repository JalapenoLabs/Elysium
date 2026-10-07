// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/satellites/{id}`: change some fields; the fleet reconnects with them.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{SatelliteResponse, SatelliteSecret, validate_http_scheme, validate_not_blank};
use crate::errors::ApiError;
use crate::models::satellite::{self, SatelliteChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Changes to a satellite. Absent fields stay as they are.
#[derive(Debug, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateSatelliteRequest {
    /// A new name.
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: Option<String>,
    /// A new description.
    #[validate(length(max = 2000))]
    description: Option<String>,
    /// A new address for the satellite's API, over http or https.
    #[validate(url, length(max = 2048), custom(function = "validate_http_scheme"))]
    url: Option<String>,
    /// A new bearer secret, replacing the sealed one.
    #[schemars(with = "Option<String>")]
    secret: Option<SatelliteSecret>,
    /// Whether Elysium watches the satellite and may start sessions on it.
    is_active: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<UpdateSatelliteRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let satellite = update(&state, id, body).await?;
    Ok(Json(json!({ "satellite": satellite })))
}

/// Changes the satellite `id`, then reconnects to it with what changed and announces it.
///
/// # Errors
/// The request's validation errors, `400` when it changes nothing, `404` for an unknown
/// satellite, or the fleet's failure to reconnect.
pub async fn update(
    state: &AppState,
    id: Uuid,
    body: UpdateSatelliteRequest,
) -> Result<SatelliteResponse, ApiError> {
    body.validate()?;

    let changes = SatelliteChanges {
        name: body.name,
        description: body.description,
        url: body.url,
        secret: body.secret.map(|secret| secret.0),
        is_active: body.is_active,
    };

    if changes.is_empty() {
        return Err(ApiError::BadRequest(
            "request body contains no fields to update".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let satellite = satellite::update(&mut connection, &state.cipher, id, &changes).await?;
    drop(connection);

    // Every change restarts the watchers: a new URL or secret needs a new client, and
    // (de)activation starts or stops watching. The status is unknown until the next poll.
    state.fleet.reload_satellite(&satellite).await?;
    let response = SatelliteResponse::new(satellite, None);
    state
        .events
        .publish(&ServerEvent::SatelliteUpserted(response.clone()));
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_fields_are_validated_and_absent_ones_are_not() {
        let empty: UpdateSatelliteRequest = serde_json::from_value(json!({})).expect("parses");
        empty
            .validate()
            .expect("an empty body has nothing to validate");

        let bad_url: UpdateSatelliteRequest =
            serde_json::from_value(json!({ "url": "arsox:8080" })).expect("parses");
        assert!(
            bad_url
                .validate()
                .expect_err("a URL without a scheme is invalid")
                .field_errors()
                .contains_key("url")
        );
    }
}
