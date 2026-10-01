// Copyright © 2026 Jalapeno Labs

//! `POST /internal/kratos/registered`: a new identity was stored, so its account is created
//! now, in the order people finished signing up (`kratos/kratos.yml`).
//!
//! That order decides who becomes the first admin. Creating accounts on a person's first API
//! request instead would let someone who signed up second, but reached the API first, take
//! it. `crate::auth::middleware` still creates an account it finds missing, so a lost
//! webhook never leaves an identity without one.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use serde::Deserialize;
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::user::{self, Profile};
use crate::realtime::ServerEvent;
use crate::routes::v1::users::UserResponse;
use crate::state::AppState;

/// The identity, as `kratos/registered.jsonnet` shapes it.
#[derive(Debug, Deserialize)]
pub struct RequestBody {
    identity_id: Uuid,
    email: String,
    name: String,
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Json(body) = body?;
    let profile = Profile::new(body.identity_id, &body.email, &body.name);

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let person = user::provision(&mut connection, &profile).await?;

    // Admins see the new sign-up waiting without reloading.
    state
        .events
        .publish(&ServerEvent::UserUpserted(UserResponse::from(person)));
    Ok(StatusCode::NO_CONTENT)
}
