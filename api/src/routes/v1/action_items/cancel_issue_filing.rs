// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/action-items/{id}/issue-filing`: give up on the issue an item is owed
//! before it is filed, such as one its tracker keeps refusing. The item stays as it is, with no
//! issue; the item's history records the cancel.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use uuid::Uuid;

use super::{publish_item_links, publish_item_write};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::action_item_issue_filing;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let cancelled =
        action_item_issue_filing::cancel(&mut connection, id, current.actor(), now).await?;
    publish_item_write(&state.events, &mut connection, cancelled, &[], now).await?;
    publish_item_links(&state.events, &mut connection, id).await?;

    Ok(StatusCode::NO_CONTENT)
}
