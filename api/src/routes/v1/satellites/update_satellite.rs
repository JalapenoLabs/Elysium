// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/satellites/{id}`: change some fields; the fleet reconnects with them.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{SatelliteResponse, SatelliteSecret, validate_http_scheme, validate_not_blank};
use crate::errors::ApiError;
use crate::models::satellite::{self, SatelliteChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are.
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: Option<String>,
    #[validate(length(max = 2000))]
    description: Option<String>,
    #[validate(url, length(max = 2048), custom(function = "validate_http_scheme"))]
    url: Option<String>,
    secret: Option<SatelliteSecret>,
    is_active: Option<bool>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
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
    state
        .events
        .publish(&ServerEvent::SatelliteUpserted(SatelliteResponse::new(
            satellite.clone(),
            None,
        )));

    Ok(Json(
        json!({ "satellite": SatelliteResponse::new(satellite, None) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn present_fields_are_validated_and_absent_ones_are_not() {
        let empty: RequestBody = serde_json::from_value(json!({})).expect("parses");
        empty
            .validate()
            .expect("an empty body has nothing to validate");

        let bad_url: RequestBody =
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
