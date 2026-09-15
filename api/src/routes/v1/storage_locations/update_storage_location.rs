// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/storage-locations/{id}`: change some fields.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{
    STORAGE_LIMIT_MAX_BYTES, StorageAccessKey, StorageLocationResponse, validate_not_blank,
    validate_path_prefix, validate_provider,
};
use crate::errors::ApiError;
use crate::models::storage_location::{self, StorageLocationChanges, StorageProvider};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// Absent fields stay as they are. A provider replaces all of its settings together, and
/// `storageLimitBytes: null` removes the limit, which is why that field distinguishes
/// "absent" from "null".
#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: Option<String>,
    #[validate(custom(function = "validate_provider"))]
    provider: Option<StorageProvider>,
    #[validate(custom(function = "validate_path_prefix"))]
    path_prefix: Option<String>,
    #[serde(default, with = "::serde_with::rust::double_option")]
    #[validate(range(min = 1, max = STORAGE_LIMIT_MAX_BYTES))]
    #[expect(
        clippy::option_option,
        reason = "absent, null, and a limit are three distinct requests"
    )]
    storage_limit_bytes: Option<Option<i64>>,
    access_key: Option<StorageAccessKey>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let changes = StorageLocationChanges {
        name: body.name,
        provider: body.provider,
        path_prefix: body
            .path_prefix
            .map(|prefix| prefix.trim_matches('/').to_owned()),
        storage_limit_bytes: body.storage_limit_bytes,
        access_key: body.access_key.map(|access_key| access_key.0),
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
    let location = storage_location::update(&mut connection, &state.cipher, id, &changes).await?;
    drop(connection);

    state.events.publish(&ServerEvent::StorageLocationUpserted(
        StorageLocationResponse::new(location.clone()),
    ));

    Ok(Json(
        json!({ "location": StorageLocationResponse::new(location) }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_limit_distinguishes_absent_from_null() {
        let absent: RequestBody =
            serde_json::from_value(json!({ "name": "Media" })).expect("parses");
        assert_eq!(absent.storage_limit_bytes, None);

        let cleared: RequestBody =
            serde_json::from_value(json!({ "storageLimitBytes": null })).expect("parses");
        assert_eq!(cleared.storage_limit_bytes, Some(None));

        let zero: RequestBody =
            serde_json::from_value(json!({ "storageLimitBytes": 0 })).expect("parses");
        assert!(
            zero.validate()
                .expect_err("a zero limit is invalid")
                .field_errors()
                .contains_key("storage_limit_bytes")
        );
    }

    #[test]
    fn present_fields_are_validated_and_absent_ones_are_not() {
        let empty: RequestBody = serde_json::from_value(json!({})).expect("parses");
        empty
            .validate()
            .expect("an empty body has nothing to validate");

        let bad_prefix: RequestBody =
            serde_json::from_value(json!({ "pathPrefix": "../elsewhere" })).expect("parses");
        assert!(
            bad_prefix
                .validate()
                .expect_err("traversal is invalid")
                .field_errors()
                .contains_key("path_prefix")
        );
    }
}
