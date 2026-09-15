// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/storage-locations`: add a location, sealing its access key.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use validator::Validate;

use super::{
    STORAGE_LIMIT_MAX_BYTES, StorageAccessKey, StorageLocationResponse, refuse_unknown_projects,
    validate_not_blank, validate_path_prefix, validate_provider,
};
use crate::errors::ApiError;
use crate::models::project::ProjectScope;
use crate::models::storage_location::{self, NewStorageLocation, StorageProvider};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 120), custom(function = "validate_not_blank"))]
    name: String,
    #[validate(custom(function = "validate_provider"))]
    provider: StorageProvider,
    #[serde(default)]
    #[validate(custom(function = "validate_path_prefix"))]
    path_prefix: String,
    /// Absent or null for no limit.
    #[serde(default)]
    #[validate(range(min = 1, max = STORAGE_LIMIT_MAX_BYTES))]
    storage_limit_bytes: Option<i64>,
    access_key: StorageAccessKey,
    /// `"*"` for every project, or project ids; absent links none yet.
    #[serde(default = "no_projects")]
    projects: ProjectScope,
}

const fn no_projects() -> ProjectScope {
    ProjectScope::Only(Vec::new())
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let new_location = NewStorageLocation {
        name: body.name,
        provider: body.provider,
        path_prefix: body.path_prefix.trim_matches('/').to_owned(),
        storage_limit_bytes: body.storage_limit_bytes,
        access_key: body.access_key.0,
        projects: body.projects,
    };

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let location = storage_location::create(&mut connection, &state.cipher, &new_location)
        .await
        .map_err(refuse_unknown_projects)?;
    drop(connection);

    state.events.publish(&ServerEvent::StorageLocationUpserted(
        StorageLocationResponse::new(location.clone(), new_location.projects.clone()),
    ));

    Ok((
        StatusCode::CREATED,
        Json(json!({ "location": StorageLocationResponse::new(location, new_location.projects) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    fn bunny_body() -> Value {
        json!({
            "name": "Media",
            "provider": { "kind": "bunny", "zone": "elysium-files", "region": "new-york" },
            "storageLimitBytes": 50_000_000_000_i64,
            "accessKey": "zone-password",
        })
    }

    #[test]
    fn a_bunny_location_parses_with_an_empty_prefix_by_default() {
        let body = parse(bunny_body()).expect("parses");

        body.validate().expect("valid");
        assert_eq!(body.path_prefix, "");
        assert_eq!(body.storage_limit_bytes, Some(50_000_000_000));
        assert!(matches!(body.provider, StorageProvider::Bunny { .. }));
    }

    #[test]
    fn limits_must_be_positive_and_exact_in_a_browser() {
        for limit in [0, -1, STORAGE_LIMIT_MAX_BYTES + 1] {
            let mut body = bunny_body();
            body["storageLimitBytes"] = json!(limit);
            assert!(
                parse(body)
                    .expect("parses")
                    .validate()
                    .expect_err("out of range")
                    .field_errors()
                    .contains_key("storage_limit_bytes"),
                "{limit} is refused"
            );
        }
    }

    #[test]
    fn an_absent_or_null_limit_means_no_limit() {
        let mut absent = bunny_body();
        absent
            .as_object_mut()
            .expect("an object")
            .remove("storageLimitBytes");
        let mut null = bunny_body();
        null["storageLimitBytes"] = Value::Null;

        for body in [absent, null] {
            let body = parse(body).expect("parses");
            body.validate().expect("valid");
            assert_eq!(body.storage_limit_bytes, None);
        }
    }

    #[test]
    fn unknown_providers_and_regions_are_rejected_while_parsing() {
        let mut unknown_kind = bunny_body();
        unknown_kind["provider"]["kind"] = json!("dropbox");
        parse(unknown_kind).expect_err("an unknown provider");

        let mut unknown_region = bunny_body();
        unknown_region["provider"]["region"] = json!("atlantis");
        parse(unknown_region).expect_err("an unknown region");
    }

    #[test]
    fn blank_access_keys_are_rejected_and_keys_never_reach_debug_output() {
        let mut blank = bunny_body();
        blank["accessKey"] = json!("  ");
        assert!(
            parse(blank)
                .expect_err("blank access key")
                .to_string()
                .contains("must not be blank")
        );

        let body = parse(bunny_body()).expect("parses");
        assert!(!format!("{body:?}").contains("zone-password"));
    }
}
