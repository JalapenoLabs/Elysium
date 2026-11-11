// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/meeting-uploads/policy?meetingId=`: what a meeting takes, and where its recording
//! goes unless the person chooses another place, which the upload page says before any file is
//! chosen. The limits are constants in code, read here so nothing keeps a second copy.
//!
//! The storage location offered is the one the meeting's owner already keeps meetings in: for a
//! capture of a meeting, the one its first recording is kept in; otherwise, or when that meeting
//! keeps none, the one their oldest meeting source keeps recordings in. `null` when they have
//! neither, and the person chooses one.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use diesel_async::AsyncPgConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::CurrentUser;
use crate::connectors::kinds::{self, Provides};
use crate::errors::ApiError;
use crate::meeting_uploads::{FileKind, PART_BYTES, accepted_extensions};
use crate::models::{connector, meeting, meeting_capture};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PolicyQuery {
    /// The meeting a capture is uploaded to, if it is one.
    meeting_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FilePolicy {
    max_bytes: u64,
    /// Lowercase, without their dots.
    extensions: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PolicyResponse {
    part_bytes: u32,
    recording: FilePolicy,
    transcript: FilePolicy,
    default_storage_location_id: Option<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    query: Result<Query<PolicyQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let default_storage_location_id =
        default_location(&mut connection, current.id(), query.meeting_id).await?;
    let policy = PolicyResponse {
        part_bytes: PART_BYTES,
        recording: FilePolicy {
            max_bytes: FileKind::Recording.max_bytes(),
            extensions: accepted_extensions(FileKind::Recording),
        },
        transcript: FilePolicy {
            max_bytes: FileKind::Transcript.max_bytes(),
            extensions: accepted_extensions(FileKind::Transcript),
        },
        default_storage_location_id,
    };
    Ok(Json(json!({ "policy": policy })))
}

/// Where `owner` already keeps meetings: see the module's docs.
async fn default_location(
    connection: &mut AsyncPgConnection,
    owner: Uuid,
    meeting_id: Option<Uuid>,
) -> Result<Option<Uuid>, ApiError> {
    if let Some(meeting_id) = meeting_id {
        let found = meeting::find_owned(connection, meeting_id, owner).await?;
        let first = meeting_capture::first_of(connection, found.id).await?;
        if let Some(location) = first.storage_location_id {
            return Ok(Some(location));
        }
    }
    let meeting_kinds = kinds::providing(Provides::Meetings);
    let sources = connector::list_visible(connection, owner, &[]).await?;
    Ok(sources
        .into_iter()
        .filter(|source| {
            meeting_kinds
                .iter()
                .any(|kind| kind.as_str() == source.kind)
        })
        .find_map(|source| source.storage_location_id))
}
