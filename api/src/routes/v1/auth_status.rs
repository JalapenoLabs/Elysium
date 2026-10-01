// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/auth/status`: what the sign-in and sign-up pages need before anyone is
//! signed in. The only `/api/v1` route that needs no session.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use serde_json::{Value, json};

use crate::errors::ApiError;
use crate::models::user;
use crate::state::AppState;

pub async fn handle(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let settings = user::workspace_settings(&mut connection).await?;
    let has_people = user::has_people(&mut connection).await?;

    Ok(Json(json!({
        // The first person may always sign up, so a new workspace can never lock itself out.
        "signupOpen": settings.signup_open || !has_people,
        // Whoever signs up first becomes the admin; the page says so.
        "firstSignUp": !has_people,
    })))
}
