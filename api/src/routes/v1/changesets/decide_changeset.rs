// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/changesets/{id}/decide`: approve or reject operations of a pending
//! changeset, or set them back to undecided.
//!
//! `operationIds` names the operations; without it the decision is for every one, which is
//! how approve all and reject all are asked. Rejecting an operation rejects every operation
//! that depends on it, and approving one whose dependency stays rejected answers `400`.
//! Nothing is written to any item or initiative until the changeset is applied.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::publish_changeset;
use crate::errors::ApiError;
use crate::models::changeset::{self, ChangesetDecision};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    decision: ChangesetDecision,
    /// The operations decided; every one when absent.
    operation_ids: Option<Vec<Uuid>>,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    if body.operation_ids.as_ref().is_some_and(Vec::is_empty) {
        return Err(ApiError::BadRequest(
            "operationIds names no operation; leave it out to decide every one".to_owned(),
        ));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let decided = changeset::decide(&mut connection, id, body.decision, body.operation_ids).await?;
    let changeset = publish_changeset(&state.events, decided);
    Ok(Json(json!({ "changeset": changeset })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_decision_names_its_operations_or_none_for_every_one() {
        let every: RequestBody =
            serde_json::from_value(json!({ "decision": "approved" })).expect("parses");
        assert_eq!(every.decision, ChangesetDecision::Approved);
        assert_eq!(every.operation_ids, None);

        let some: RequestBody = serde_json::from_value(json!({
            "decision": "rejected",
            "operationIds": [Uuid::nil()],
        }))
        .expect("parses");
        assert_eq!(some.operation_ids, Some(vec![Uuid::nil()]));

        serde_json::from_value::<RequestBody>(json!({ "decision": "maybe" }))
            .expect_err("an unknown decision");
    }
}
