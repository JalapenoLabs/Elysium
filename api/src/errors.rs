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
    /// A service this request needs is not configured on this deployment.
    #[error("{0}")]
    Unavailable(&'static str),
    /// An upstream (a satellite, a mail server, the OAuth broker) refused the request
    /// or could not be reached. The message is the upstream's own error, which never
    /// carries a secret.
    #[error("{0}")]
    BadGateway(String),
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

impl From<arsox_sdk::client::Error> for ApiError {
    fn from(error: arsox_sdk::client::Error) -> Self {
        Self::BadGateway(error.to_string())
    }
}

impl From<crate::mail::transport::MailError> for ApiError {
    fn from(error: crate::mail::transport::MailError) -> Self {
        Self::BadGateway(error.to_string())
    }
}

impl From<crate::mail::broker::BrokerError> for ApiError {
    fn from(error: crate::mail::broker::BrokerError) -> Self {
        use crate::mail::broker::BrokerError;

        match error {
            BrokerError::InvalidGrant(message) => {
                Self::BadGateway(format!("the account must be connected again: {message}"))
            }
            BrokerError::Unavailable(message) => {
                Self::BadGateway(format!("OAuth broker: {message}"))
            }
            not_oauth @ BrokerError::NotOAuth(_) => Self::Internal(not_oauth.into()),
        }
    }
}

impl From<crate::mail::stalwart::StalwartError> for ApiError {
    fn from(error: crate::mail::stalwart::StalwartError) -> Self {
        use crate::mail::stalwart::StalwartError;

        match error {
            StalwartError::AddressTaken => {
                Self::Conflict("the mail server already has an account for that address")
            }
            refused @ StalwartError::Refused(_) => Self::BadGateway(refused.to_string()),
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
            Self::Unavailable(message) => (
                StatusCode::SERVICE_UNAVAILABLE,
                json!({ "message": message }),
            ),
            Self::BadGateway(message) => {
                event!(
                    name: "http.upstream.failure",
                    Level::WARN,
                    error.message = %message,
                    "an upstream call failed",
                );
                (StatusCode::BAD_GATEWAY, json!({ "message": message }))
            }
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
