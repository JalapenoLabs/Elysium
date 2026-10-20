// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/elysia/questions/{id}/dismiss`: a person closes one of Elysia's questions
//! without answering it. Nothing is queued; the work it was about stays where it stands until
//! her idle loop comes back to it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use serde_json::{Value, json};
use uuid::Uuid;

use super::{QuestionResponse, publish_snapshot};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::elysia;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Some(question) = elysia::close_question(&mut connection, id, None, current.id()).await?
    else {
        return Err(ApiError::CodedConflict {
            code: "question_not_open",
            message: "the question was already answered or dismissed",
        });
    };

    publish_snapshot(&state.events, &mut connection).await?;
    Ok(Json(
        json!({ "question": QuestionResponse::from(question) }),
    ))
}
