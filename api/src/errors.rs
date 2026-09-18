// Copyright © 2026 Jalapeno Labs

//! The error type every handler returns, and how each case becomes a response.
//!
//! Client mistakes carry a specific message. Server faults are logged in full and
//! answered with a generic message so internals never leak into a response.

use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
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
    /// The request named something the credential it was made with may not touch, such as
    /// a Jira project outside its allowlist. The message names what and whose.
    #[error("{0}")]
    Forbidden(String),
    #[error("resource not found")]
    NotFound,
    #[error("{0}")]
    Conflict(&'static str),
    /// A service this request needs is not configured on this deployment.
    #[error("{0}")]
    Unavailable(&'static str),
    /// An upstream (a satellite, a mail server, the OAuth broker, a storage provider)
    /// refused the request or could not be reached. The message is the upstream's own
    /// error, which never carries a secret.
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

impl From<QueryRejection> for ApiError {
    fn from(rejection: QueryRejection) -> Self {
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

impl From<crate::github::GithubError> for ApiError {
    fn from(error: crate::github::GithubError) -> Self {
        use crate::github::GithubError;

        match error {
            // A token GitHub will not accept is the client's to fix, not an upstream fault.
            GithubError::Unauthorized(message) => Self::BadRequest(message.to_owned()),
            refused @ GithubError::Refused(_) => Self::BadGateway(refused.to_string()),
        }
    }
}

impl From<crate::jira::JiraError> for ApiError {
    fn from(error: crate::jira::JiraError) -> Self {
        use crate::jira::JiraError;

        match error {
            // Credentials Jira will not accept, and a call Jira reads as malformed, are
            // both the client's to fix rather than an upstream fault.
            JiraError::Unauthorized(message) => Self::BadRequest(message.to_owned()),
            invalid @ JiraError::Invalid(_) => Self::BadRequest(invalid.to_string()),
            JiraError::NotFound => Self::NotFound,
            refused @ JiraError::Refused(_) => Self::BadGateway(refused.to_string()),
        }
    }
}

impl From<crate::images::ImageError> for ApiError {
    fn from(error: crate::images::ImageError) -> Self {
        use crate::images::ImageError;

        match error {
            // Encoding pixels that decoded fine is not the client's fault.
            encoding @ ImageError::Encoding(_) => Self::Internal(encoding.into()),
            refused => Self::BadRequest(refused.to_string()),
        }
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
            // Only setup signs in with a password someone typed, and it maps this itself.
            // Anywhere else the stored administrator was refused: Stalwart lost its data.
            StalwartError::Unauthorized => Self::BadGateway(
                "the mail server rejected Elysium's administrator; set the mail server up again"
                    .to_owned(),
            ),
            refused @ StalwartError::Refused(_) => Self::BadGateway(refused.to_string()),
        }
    }
}

impl From<crate::storage::StorageError> for ApiError {
    fn from(error: crate::storage::StorageError) -> Self {
        use crate::storage::StorageError;

        match error {
            StorageError::NotFound => Self::NotFound,
            // A path that is not plain, or an upload too large, is the client's to fix.
            invalid @ StorageError::Invalid(_) => Self::BadRequest(invalid.to_string()),
            upstream @ (StorageError::Unauthorized(_) | StorageError::Refused(_)) => {
                Self::BadGateway(upstream.to_string())
            }
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
            Self::Forbidden(message) => (StatusCode::FORBIDDEN, json!({ "message": message })),
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
