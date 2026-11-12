// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/meeting-attachments/limit`: an admin chooses the largest file a meeting takes,
//! from one byte to what a storage provider takes in one upload. It bounds what is attached from
//! now on; a file attached already stays, and an upload under way is held to the limit it started
//! under, since it was checked as it started.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{Level, event};

use super::PolicyResponse;
use crate::auth::AdminUser;
use crate::errors::ApiError;
use crate::meeting_attachments::MAX_BYTES_CEILING;
use crate::models::user::{self, WorkspaceSettingsChanges};
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    max_bytes: u64,
}

pub async fn handle(
    State(state): State<AppState>,
    admin: AdminUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    if body.max_bytes == 0 || body.max_bytes > MAX_BYTES_CEILING {
        return Err(ApiError::CodedBadRequest {
            code: "meeting_attachment_limit",
            message: format!("the largest file is 1 to {MAX_BYTES_CEILING} bytes"),
        });
    }
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let changes = WorkspaceSettingsChanges {
        meeting_attachment_max_bytes: Some(
            i64::try_from(body.max_bytes).context("the ceiling fits a bigint")?,
        ),
        ..WorkspaceSettingsChanges::default()
    };
    let settings = user::update_workspace_settings(&mut connection, changes, admin.id()).await?;
    drop(connection);

    event!(
        name: "meetings.attachments.limit.changed",
        Level::INFO,
        file.size = body.max_bytes,
        "chose the largest file a meeting takes: {{file.size}} bytes",
    );
    state.events.publish(&ServerEvent::MeetingAttachmentsChanged);
    Ok(Json(json!({ "policy": PolicyResponse::from(settings) })))
}
