// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/users/{id}/approve`: let a pending person in, with a role.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::UserResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::models::user::{self, PersonRole};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    role: PersonRole,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let approved = user::approve(&mut connection, id, body.role, admin.id()).await?;

    state
        .events
        .publish(&ServerEvent::UserUpserted(UserResponse::from(
            approved.clone(),
        )));
    Ok(Json(json!({ "user": UserResponse::from(approved) })))
}
