// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/me`: the person signed in, and what stands between them and the workspace.
//!
//! The one workspace route a pending, disabled, or not-yet-enrolled person may reach, so the
//! frontend can show them why they are waiting. The pending page asks it every few seconds
//! to notice an approval.

use anyhow::Context;
use axum::Extension;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use super::users::UserResponse;
use crate::auth::Principal;
use crate::auth::kratos::AssuranceLevel;
use crate::errors::ApiError;
use crate::models::user;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    Extension(principal): Extension<Principal>,
) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let settings = user::workspace_settings(&mut connection).await?;
    let has_second_factor = principal.assurance >= AssuranceLevel::Aal2;

    Ok(Json(json!({
        "user": UserResponse::from(principal.user),
        "hasSecondFactor": has_second_factor,
        "mfaEnrollmentRequired": settings.require_mfa && !has_second_factor,
    })))
}
