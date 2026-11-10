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
use serde_json::{Value, json};
use tracing::{Level, event};
use validator::ValidationErrors;

/// Handler failure, mapped to an HTTP status by [`IntoResponse`].
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("{0}")]
    BadRequest(String),
    /// A request the frontend explains in its own words, named by a stable `code`, such as a
    /// GitHub token that may not file a report.
    #[error("{message}")]
    CodedBadRequest { code: &'static str, message: String },
    /// A body over what its route takes, such as a file too large to attach. Named by a stable
    /// `code`, since the web app says the limit in its own words.
    #[error("{message}")]
    PayloadTooLarge { code: &'static str, message: String },
    /// A person past a limit the code sets, such as the reports they may file a day. Named by a
    /// stable `code`; the message says when the limit lifts.
    #[error("{message}")]
    TooManyRequests { code: &'static str, message: String },
    #[error("request failed validation")]
    Validation(#[from] ValidationErrors),
    /// The request named something the credential it was made with may not touch, such as
    /// a Jira project outside its allowlist. The message names what and whose.
    #[error("{0}")]
    Forbidden(String),
    /// Who is asking may not: nobody is signed in, or this person may not do this. The body
    /// carries a stable `code` the frontend acts on.
    #[error("{}", .0.message())]
    AccessRefused(crate::auth::AccessRefusal),
    #[error("resource not found")]
    NotFound,
    #[error("{0}")]
    Conflict(&'static str),
    /// A conflict the frontend explains in its own words, named by a stable `code`.
    #[error("{message}")]
    CodedConflict {
        code: &'static str,
        message: &'static str,
    },
    /// A service this request needs is not configured on this deployment, or cannot serve it
    /// right now, such as the event stream while its Redis subscription reconnects.
    #[error("{0}")]
    Unavailable(&'static str),
    /// An upstream (the workspace, a mail server, the OAuth broker, a storage provider)
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

impl From<crate::action_items::WorkError> for ApiError {
    fn from(error: crate::action_items::WorkError) -> Self {
        use crate::action_items::WorkError;

        match error {
            WorkError::Database(database) => database.into(),
            WorkError::Conflict(message) => Self::Conflict(message),
            WorkError::Invalid(message) => Self::BadRequest(message.to_owned()),
            WorkError::Refused(message) => Self::BadRequest(message),
        }
    }
}

impl From<crate::models::work_batch::ChangeError> for ApiError {
    fn from(error: crate::models::work_batch::ChangeError) -> Self {
        use crate::models::work_batch::ChangeError;

        match error {
            ChangeError::NotFound => Self::NotFound,
            ChangeError::Refused(_reason) => Self::CodedConflict {
                code: "work_batch_moved_on",
                message: "this batch moved on, so it no longer allows that change",
            },
            ChangeError::Invalid(reason) => Self::BadRequest(reason),
            ChangeError::Database(database) => database.into(),
        }
    }
}

impl From<crate::findings::FindingError> for ApiError {
    fn from(error: crate::findings::FindingError) -> Self {
        use crate::findings::FindingError;
        use crate::models::finding::DecideError;

        match error {
            FindingError::Invalid(reason) => Self::BadRequest(reason),
            FindingError::Decide(DecideError::NotFound) => Self::NotFound,
            FindingError::Decide(DecideError::Decided(_state)) => Self::CodedConflict {
                code: "finding_decided",
                message: "someone already decided this finding",
            },
            FindingError::Decide(DecideError::Database(database))
            | FindingError::Database(database) => database.into(),
            FindingError::Work(work) => work.into(),
        }
    }
}

impl From<crate::models::attribution::UndoError> for ApiError {
    fn from(error: crate::models::attribution::UndoError) -> Self {
        use crate::models::attribution::UndoError;

        match error {
            UndoError::NotFound => Self::NotFound,
            UndoError::ChangedSince => Self::CodedConflict {
                code: "attribution_changed",
                message: "this attribution changed since, so undoing it would overwrite that",
            },
            UndoError::Database(database) => database.into(),
        }
    }
}

impl From<crate::models::learning::LearningError> for ApiError {
    fn from(error: crate::models::learning::LearningError) -> Self {
        use crate::models::learning::LearningError;

        match error {
            LearningError::Database(database) => database.into(),
            LearningError::TitleTaken => Self::Conflict("another learning already has this title"),
            LearningError::Deleted => {
                Self::Conflict("this learning is deleted; restore it to change it")
            }
            LearningError::NothingToUndo(message) => Self::Conflict(message),
            unknown @ LearningError::UnknownCategory { .. } => {
                Self::BadRequest(unknown.to_string())
            }
            LearningError::Page(page) => page.into(),
        }
    }
}

impl From<crate::models::page::PageError> for ApiError {
    fn from(error: crate::models::page::PageError) -> Self {
        use crate::models::page::PageError;

        // Each is a change the page's state refuses, so a conflict rather than `403`, which says
        // who is asking may not. The web app explains each in its own words.
        let (code, message) = match error {
            PageError::Database(database) => return database.into(),
            PageError::Changed => (
                "page_changed",
                "the page changed since this was opened; read it again with page_read, make \
                 the change on what it says now, and save it against the currentVersion that \
                 answers",
            ),
            PageError::Deleted => (
                "page_deleted",
                "this page is deleted; restore it to change it",
            ),
            PageError::ProposalOnly => (
                "page_proposal_only",
                "people edit this page directly; agents propose a change to a section of the \
                 company doc with company_doc_propose",
            ),
            PageError::ParentGone => (
                "page_parent_gone",
                "the page it would go under does not exist or is deleted",
            ),
            PageError::PlacementGone => (
                "page_placement_gone",
                "the page it would follow is no longer under that parent; load the tree again",
            ),
            PageError::UnderItself => ("page_under_itself", "a page cannot move under itself"),
            PageError::Fixed => (
                "page_fixed",
                "this page holds its place: it cannot be moved, deleted, or renamed",
            ),
            PageError::TitleTaken => (
                "page_title_taken",
                "another learning already has this title",
            ),
            PageError::NotALearning => (
                "page_not_a_learning",
                "only a learning has a category and evidence",
            ),
            PageError::TooManyLabels { .. } => (
                "page_label_limit",
                "this page carries as many labels as a page may; remove one first",
            ),
            PageError::TooManyAttachments { .. } => (
                "page_attachment_limit",
                "this page holds as many attachments as a page may; remove one first",
            ),
            PageError::NoAttachmentLocation => (
                "attachment_no_location",
                "no storage location is chosen for attachments; an admin chooses one in Settings, \
                 Storage",
            ),
            refused @ (PageError::UnknownCategory { .. } | PageError::BodyOutOfBounds { .. }) => {
                return Self::BadRequest(refused.to_string());
            }
        };
        Self::CodedConflict { code, message }
    }
}

impl From<crate::models::company_doc::CompanyDocError> for ApiError {
    fn from(error: crate::models::company_doc::CompanyDocError) -> Self {
        use crate::models::company_doc::CompanyDocError;

        match error {
            CompanyDocError::Database(database) => database.into(),
            CompanyDocError::Page(page) => page.into(),
            refused @ (CompanyDocError::Invalid(_)
            | CompanyDocError::TooLong(_)
            | CompanyDocError::EditDoesNotApply(_)
            | CompanyDocError::ChangesNothing(_)) => Self::BadRequest(refused.to_string()),
            CompanyDocError::AlreadyDecided(_) => {
                Self::Conflict("this proposal was already decided")
            }
            // The web app answers this by saying the proposal no longer fits the section.
            CompanyDocError::Overlaps(_) => Self::CodedConflict {
                code: "proposal_overlaps",
                message: "the section changed since this was proposed, in the same lines; \
                    reject it, or make the change by hand",
            },
            CompanyDocError::NothingToUndo(message) => Self::Conflict(message),
        }
    }
}

impl From<crate::agent_guidance::GuidanceError> for ApiError {
    fn from(error: crate::agent_guidance::GuidanceError) -> Self {
        use crate::agent_guidance::GuidanceError;

        match error {
            GuidanceError::Database(database) => database.into(),
            too_long @ GuidanceError::TooLong { .. } => Self::BadRequest(too_long.to_string()),
            // The web app answers this by saying another admin saved meanwhile.
            GuidanceError::Changed => Self::CodedConflict {
                code: "guidance_changed",
                message: "the guidance changed since this was opened; load it again, then make \
                    the change",
            },
        }
    }
}

impl From<crate::models::learning_category::CategoryError> for ApiError {
    fn from(error: crate::models::learning_category::CategoryError) -> Self {
        use crate::models::learning_category::CategoryError;

        match error {
            CategoryError::Database(database) => database.into(),
            CategoryError::Learning(learning) => learning.into(),
            CategoryError::KeyTaken => Self::Conflict("a category already has this key"),
            CategoryError::BuiltIn => {
                Self::Conflict("Elysium's built-in categories cannot be changed or removed")
            }
            // The web app answers this by asking where the learnings should go.
            CategoryError::InUse => Self::CodedConflict {
                code: "category_in_use",
                message: "learnings are filed under this category; choose a category to move \
                    them to",
            },
            CategoryError::InvalidMove(message) => Self::BadRequest(message.to_owned()),
        }
    }
}

impl From<crate::agent_models::ChangeError> for ApiError {
    fn from(error: crate::agent_models::ChangeError) -> Self {
        use crate::agent_models::ChangeError;

        match error {
            ChangeError::Unknown(_) => Self::NotFound,
            ChangeError::RunsNoModel(_) => {
                Self::Conflict("this step runs no model, so it has no model setting")
            }
            ChangeError::NotInProjects(_) => Self::CodedConflict {
                code: "kind_not_in_projects",
                message: "this kind of agent works for the whole workspace; set it under \
                    Settings, Models",
            },
            ChangeError::Database(database) => database.into(),
        }
    }
}

impl From<crate::models::step_job::StepActionError> for ApiError {
    fn from(error: crate::models::step_job::StepActionError) -> Self {
        use crate::models::step_job::StepActionError;

        match error {
            StepActionError::Database(database) => database.into(),
            StepActionError::Refused(message) => Self::Conflict(message),
        }
    }
}

impl From<crate::models::user::AccountError> for ApiError {
    fn from(error: crate::models::user::AccountError) -> Self {
        use crate::models::user::AccountError;

        match error {
            AccountError::Database(database) => database.into(),
            AccountError::LastAdmin => Self::CodedConflict {
                code: "last_admin",
                message: "the workspace must keep at least one active admin",
            },
            AccountError::Invalid(message) => Self::CodedConflict {
                code: "account_state",
                message,
            },
        }
    }
}

impl From<crate::auth::kratos::KratosError> for ApiError {
    fn from(error: crate::auth::kratos::KratosError) -> Self {
        use crate::auth::AccessRefusal;
        use crate::auth::kratos::KratosError;

        match error {
            KratosError::NotFound => Self::NotFound,
            KratosError::NoSession => Self::AccessRefused(AccessRefusal::Unauthenticated),
            KratosError::SecondFactorRequired => {
                Self::AccessRefused(AccessRefusal::SecondFactorRequired)
            }
            refused @ KratosError::Refused(_) => Self::BadGateway(refused.to_string()),
        }
    }
}

impl From<crate::oauth::hydra::HydraError> for ApiError {
    fn from(error: crate::oauth::hydra::HydraError) -> Self {
        use crate::oauth::hydra::HydraError;

        match error {
            HydraError::NotFound => Self::NotFound,
            refused @ HydraError::Refused(_) => Self::BadGateway(refused.to_string()),
        }
    }
}

impl From<crate::runner::files::FileError> for ApiError {
    fn from(error: crate::runner::files::FileError) -> Self {
        use crate::runner::files::FileError;

        match error {
            refused @ (FileError::NotFound(_) | FileError::Outside(_)) => {
                Self::BadRequest(refused.to_string())
            }
            failed @ (FileError::Failed(_) | FileError::Workspace(_)) => {
                Self::BadGateway(format!("the workspace: {failed}"))
            }
        }
    }
}

impl From<crate::meeting_uploads::staging::StagingError> for ApiError {
    fn from(error: crate::meeting_uploads::staging::StagingError) -> Self {
        use crate::meeting_uploads::staging::StagingError;

        match error {
            short @ StagingError::Length { .. } => Self::CodedBadRequest {
                code: "meeting_upload_part_length",
                message: short.to_string(),
            },
            StagingError::Workspace(refused) => Self::from(refused),
            failed @ StagingError::Io(_) => Self::BadGateway(failed.to_string()),
        }
    }
}

impl From<crate::github::GithubError> for ApiError {
    fn from(error: crate::github::GithubError) -> Self {
        use crate::github::GithubError;

        match error {
            // A token GitHub will not accept is the client's to fix, not an upstream fault.
            GithubError::Unauthorized(message) => Self::BadRequest(message.to_owned()),
            refused @ (GithubError::NoAccess(_) | GithubError::Refused(_)) => {
                Self::BadGateway(refused.to_string())
            }
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

impl ApiError {
    /// The status and JSON body a client is answered with, logging what only the server keeps.
    ///
    /// Both the HTTP routes and the MCP server report failures through this, so a tool result
    /// says exactly what the browser would be told and an internal error stays as generic.
    pub fn into_status_and_body(self) -> (StatusCode, Value) {
        match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, json!({ "message": message })),
            Self::CodedBadRequest { code, message } => (
                StatusCode::BAD_REQUEST,
                json!({ "message": message, "code": code }),
            ),
            Self::PayloadTooLarge { code, message } => (
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({ "message": message, "code": code }),
            ),
            Self::TooManyRequests { code, message } => (
                StatusCode::TOO_MANY_REQUESTS,
                json!({ "message": message, "code": code }),
            ),
            Self::Validation(errors) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!({ "message": "request failed validation", "fields": errors }),
            ),
            Self::Forbidden(message) => (StatusCode::FORBIDDEN, json!({ "message": message })),
            Self::AccessRefused(refusal) => (
                refusal.status(),
                json!({ "message": refusal.message(), "code": refusal.code() }),
            ),
            Self::NotFound => (
                StatusCode::NOT_FOUND,
                json!({ "message": "resource not found" }),
            ),
            Self::Conflict(message) => (StatusCode::CONFLICT, json!({ "message": message })),
            Self::CodedConflict { code, message } => (
                StatusCode::CONFLICT,
                json!({ "message": message, "code": code }),
            ),
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
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = self.into_status_and_body();
        (status, Json(body)).into_response()
    }
}
