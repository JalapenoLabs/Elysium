// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/meetings/duplicates/{id}/merge` and `POST .../not-the-same`: the signed-in person
//! answers an inbox item asking whether one of their meetings is the same as another. Merge moves
//! its captures into the other and lines that one up again; Not the same is remembered. Either
//! resolves the item and lets the meeting's waiting run go on. `404` for a pair that is not
//! theirs, `409` once it is decided. See `crate::meetings::duplicates`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::get_duplicate;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::meetings::duplicates::{self, DecideError};
use crate::state::AppState;

pub async fn merge(
    state: State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    decide(state, current, path, Answer::Merge).await
}

pub async fn not_the_same(
    state: State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    decide(state, current, path, Answer::NotTheSame).await
}

/// What the person answered.
#[derive(Debug, Clone, Copy)]
enum Answer {
    Merge,
    NotTheSame,
}

async fn decide(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    answer: Answer,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let now = Utc::now();
    let decided = match answer {
        Answer::Merge => duplicates::merge(&mut connection, id, current.id(), now).await,
        Answer::NotTheSame => {
            duplicates::not_the_same(&mut connection, id, current.id(), now).await
        }
    }
    .map_err(|error| match error {
        DecideError::NotFound => ApiError::NotFound,
        DecideError::Decided => ApiError::CodedConflict {
            code: "duplicate_decided",
            message: "Whether these are the same meeting was decided already.",
        },
        DecideError::Database(database) => database.into(),
        DecideError::Other(other) => other.into(),
    })?;
    let pair = decided.pair.clone();
    duplicates::announce(&state.events, &mut connection, decided).await;
    let answered = get_duplicate::response(&mut connection, pair).await?;
    Ok(Json(json!({ "duplicate": answered })))
}
