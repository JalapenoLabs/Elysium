// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/updates/apply`: asks for the newest release, even one that rolled back here
//! before. The updater applies it once nothing is running that a restart would cut short.

use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::state::AppState;
use crate::updates::RequestError;

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
) -> Result<Json<Value>, ApiError> {
    state
        .updates
        .request(admin.id())
        .await
        .map_err(|error| match error {
            RequestError::NotARelease => ApiError::CodedConflict {
                code: "not_a_release",
                message: "this build is not a release, so it does not update itself",
            },
            RequestError::NothingNewer => ApiError::CodedConflict {
                code: "nothing_newer",
                message: "no newer release is known; check for one first",
            },
        })?;
    let status = state.updates.status().await?;
    Ok(Json(json!({ "updates": status })))
}
