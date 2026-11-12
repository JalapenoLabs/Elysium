// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/knowledge/meetings?from=&to=&person=&project=&limit=`: the meetings the
//! knowledge base holds, newest first, each with its excerpts as citations (`topics`) in the order
//! they were said. Whoever's meeting source recorded one, every member reads it, as they read its
//! excerpts through the search. See `crate::models::knowledge_meeting`.
//!
//! The work is [`list`], which the MCP server's `meetings_list` and the agents' tool of the same
//! name call too, with [`ListMeetingsRequest`] as their arguments.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::knowledge_meeting::{self, MeetingFilters, MeetingView};
use crate::state::AppState;

/// Meetings when the caller names no limit: about a fortnight of a busy calendar.
const DEFAULT_LIMIT: i64 = 20;

/// The most meetings one listing answers. Each carries its topics' citations, not their text,
/// so fifty stay a small answer.
const MAX_LIMIT: i64 = 50;

/// Which meetings to list. Every filter narrows; none hides a meeting from a member.
#[derive(Debug, Default, Deserialize, Validate, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[validate(schema(function = "validate_period"))]
pub struct ListMeetingsRequest {
    /// Only meetings that started at or after this moment, with an offset, such as
    /// `2026-11-04T09:00:00Z`.
    pub from: Option<DateTime<Utc>>,
    /// Only meetings that started before this moment, with an offset.
    pub to: Option<DateTime<Utc>>,
    /// Only meetings recorded by this person's meeting sources, by their user id.
    pub person: Option<Uuid>,
    /// Only meetings with at least one topic attributed to this project, by its id.
    pub project: Option<Uuid>,
    /// How many meetings to answer, newest first, from 1 to 50. Defaults to 20.
    #[validate(range(min = 1, max = MAX_LIMIT))]
    pub limit: Option<i64>,
}

/// A period that ends where it starts, or before, holds nothing, which is a mistake to say
/// rather than answer as no meetings.
fn validate_period(request: &ListMeetingsRequest) -> Result<(), ValidationError> {
    match (request.from, request.to) {
        (Some(from), Some(to)) if from >= to => {
            Err(ValidationError::new("period").with_message("`from` must be before `to`".into()))
        }
        _ => Ok(()),
    }
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    query: Result<Query<ListMeetingsRequest>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(request) = query?;
    let meetings = list(&state.database, request).await?;
    Ok(Json(json!({ "meetings": meetings })))
}

/// The meetings `request` names, newest first.
///
/// It answers any person's meetings: callers check that whoever asks may read the knowledge base
/// (`docs/knowledge.md`, Who sees what).
///
/// # Errors
/// [`ApiError::Validation`] for arguments outside their limits, and an internal error when the
/// database does not answer.
pub async fn list(
    database: &Pool,
    request: ListMeetingsRequest,
) -> Result<Vec<MeetingView>, ApiError> {
    request.validate()?;
    let filters = MeetingFilters {
        person_id: request.person,
        project_id: request.project,
        started_from: request.from,
        started_before: request.to,
    };
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let meetings = knowledge_meeting::list(
        &mut connection,
        &filters,
        request.limit.unwrap_or(DEFAULT_LIMIT),
    )
    .await
    .context("cannot list the knowledge base's meetings")?;
    Ok(meetings)
}
