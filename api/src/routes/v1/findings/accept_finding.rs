// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/findings/{id}/accept`: a person accepts an open finding. It becomes an open action
//! item in its project, as theirs, whose issue is owed in the project's tracker, filed as them, and
//! a batch for that item, human-rooted and generation 0, which waits for approval or queues at once
//! as the project's level of autonomy says, like any batch that traces to a person. Answers the
//! finding, the item's id, the batch's id and state, and where the issue files, or `issue: null`
//! with `issueRefusal` saying why it files nowhere.
//! `409` (`finding_decided`) once someone decided it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use super::FindingResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::findings::{self, Accepted};
use crate::project_managers::batches;
use crate::realtime::Signal;
use crate::routes::v1::action_items::publish_item_write;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    person: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Accepted {
        finding,
        item,
        batch,
        issue,
    } = findings::accept(&mut connection, id, &person, now).await?;

    findings::publish(&state.events, &finding);
    let item_id = item.record.id;
    publish_item_write(&state.events, &mut connection, item, &[], now).await?;
    let batch = batches::announce(&mut connection, &state.events, batch).await;
    let (issue, issue_refusal) = match issue {
        Ok(place) => {
            // The filing is owed; the watcher lands it now rather than at its next pass.
            state.events.signal(&Signal::LinkWatcher);
            (
                Some(json!({ "provider": place.provider, "destination": place.destination })),
                None,
            )
        }
        Err(reason) => (None, Some(reason)),
    };
    Ok(Json(json!({
        "finding": FindingResponse::from(finding),
        "itemId": item_id,
        "batchId": batch.id,
        "batchState": batch.state,
        "issue": issue,
        "issueRefusal": issue_refusal,
    })))
}
