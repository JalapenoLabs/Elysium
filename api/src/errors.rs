// Copyright © 2026 Jalapeno Labs

//! The error type every handler returns, and how each case becomes a response.
//!
//! Client mistakes carry a specific message. Server faults are logged in full and
//! answered with a generic message so internals never leak into a response.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde_json::json;
use tracing::{Level, event};
use validator::ValidationErrors;

/// Handler failure, mapped to an HTTP status by [`IntoResponse`].
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    #[error("request failed validation")]
    Validation(#[from] ValidationErrors),
    #[error("resource not found")]
    NotFound,
    #[error("{0}")]
    Conflict(&'static str),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

impl From<JsonRejection> for ApiError {
    fn from(rejection: JsonRejection) -> Self {
        Self::BadRequest(rejection.body_text())
    }
}

impl From<PathRejection> for ApiError {
    fn from(rejection: PathRejection) -> Self {
        Self::BadRequest(rejection.body_text())
    }
}

impl From<DieselError> for ApiError {
    fn from(error: DieselError) -> Self {
        match error {
            DieselError::NotFound => Self::NotFound,
            DieselError::DatabaseError(DatabaseErrorKind::UniqueViolation, _) => {
                Self::Conflict("a record with the same unique value already exists")
            }
            other => Self::Internal(other.into()),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, json!({ "message": message })),
            Self::Validation(errors) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "message": "request failed validation", "fields": errors }),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                json!({ "message": "resource not found" }),
            ),
            Self::Conflict(message) => (StatusCode::CONFLICT, json!({ "message": message })),
            Self::Internal(error) => {
                event!(
                    name: "http.handler.failure",
                    Level::ERROR,
                    error.message = %error,
                    error.chain = ?error,
                    "request failed with an internal error",
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({ "message": "internal server error" }),
                )
            }
        };

        (status, Json(body)).into_response()
    }
}
