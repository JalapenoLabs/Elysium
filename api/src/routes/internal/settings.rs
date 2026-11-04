// Copyright © 2026 Jalapeno Labs

//! `POST /internal/kratos/settings`: a person changed their settings in Kratos, so every session
//! of theirs the API cached is forgotten (`kratos/kratos.yml`, `settings.after`).
//!
//! Kratos changes sessions behind the API's back here: a new password signs every other session
//! out (`revoke_active_sessions`), and a new authenticator changes what a session must have done
//! to be let in. A new name or email is in the cached answer too. Forgetting makes each session's
//! next request ask Kratos as it now stands.
//!
//! Kratos calls this once the change is stored, and waits for it, so the person's next request
//! never sees the old answer. It always answers `204`: the change has landed, and a cache that
//! could not be told is logged as an error where it fails.

use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::auth::sessions;
use crate::errors::ApiError;
use crate::state::AppState;

/// The identity whose settings changed, as `kratos/settings.jsonnet` shapes it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestBody {
    identity_id: Uuid,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;
    sessions::forget(&state.redis, body.identity_id).await;
    Ok(StatusCode::NO_CONTENT)
}
