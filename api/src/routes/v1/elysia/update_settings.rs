// Copyright © 2026 Jalapeno Labs

//! `PATCH /api/v1/elysia/settings`: pause or resume Elysia, set the hours she may start work
//! on her own, or lift a usage-limit hold early. Admins only, since she acts for the whole
//! workspace.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};

use super::publish_snapshot;
use crate::auth::AdminUser;
use crate::elysia::availability::{self, AvailabilityWindow};
use crate::errors::ApiError;
use crate::models::elysia::{self, ElysiaStateChanges};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[serde(default)]
    paused: Option<bool>,
    /// UTC minute-of-week windows; an empty list means any time.
    #[serde(default)]
    availability: Option<Vec<AvailabilityWindow>>,
    /// `true` lifts a usage-limit hold now, such as after a credential was added.
    #[serde(default)]
    resume_usage: bool,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    if let Some(windows) = &body.availability {
        availability::validate(windows).map_err(|error| ApiError::BadRequest(error.to_string()))?;
    }
    let availability = body
        .availability
        .map(serde_json::to_value)
        .transpose()
        .context("availability windows serialize")?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    elysia::update_state(
        &mut connection,
        ElysiaStateChanges {
            paused: body.paused,
            availability,
        },
        admin.id(),
    )
    .await?;
    if body.resume_usage {
        elysia::set_usage_paused_until(&mut connection, None).await?;
    }

    let response = publish_snapshot(&state.events, &mut connection).await?;
    Ok(Json(json!({ "elysia": response })))
}
