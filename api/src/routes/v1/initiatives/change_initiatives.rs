// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/initiatives/bulk`: apply one change to many initiatives at once, as a
//! person's multi-select asks: a state, deleting, or restoring.
//!
//! Each initiative goes through the write its own route makes, in one transaction, and
//! records its own history with the person as the actor
//! (`crate::models::initiative_bulk`). One the records refuse is answered with the reason
//! while the rest apply, so the answer is `200` listing both.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{publish_initiative_write, publish_member_items};
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::initiative_bulk::{self, BulkInitiativeChange, MAX_INITIATIVES};
use crate::routes::v1::action_items::unique_ids;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    /// Each initiative to change, once or more; repeats count once.
    initiative_ids: Vec<Uuid>,
    change: BulkInitiativeChange,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Json(body) = body?;
    let now = Utc::now();

    let initiative_ids = unique_ids(body.initiative_ids);
    if initiative_ids.is_empty() {
        return Err(ApiError::BadRequest(
            "initiativeIds must name at least one initiative".to_owned(),
        ));
    }
    if initiative_ids.len() > MAX_INITIATIVES {
        return Err(ApiError::BadRequest(format!(
            "initiativeIds may name at most {MAX_INITIATIVES} initiatives at once"
        )));
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let outcome = initiative_bulk::change_many(
        &mut connection,
        &initiative_ids,
        body.change,
        current.actor(),
        now,
    )
    .await?;

    // Deleting or restoring changes which initiatives the members list, so they go out too.
    let moves_members = matches!(
        body.change,
        BulkInitiativeChange::Delete {} | BulkInitiativeChange::Restore {}
    );
    let mut initiatives = Vec::with_capacity(outcome.written.len());
    for written in outcome.written {
        let id = written.record.id;
        let is_changed = !written.history.is_empty();
        initiatives
            .push(publish_initiative_write(&state.events, &mut connection, written, now).await?);
        if moves_members && is_changed {
            publish_member_items(&state.events, &mut connection, id, now).await?;
        }
    }

    Ok(Json(
        json!({ "initiatives": initiatives, "refused": outcome.refused }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_names_initiatives_and_one_change() {
        let initiative_id = Uuid::now_v7();
        let body: RequestBody = serde_json::from_value(json!({
            "initiativeIds": [initiative_id],
            "change": { "kind": "state", "state": "abandoned" },
        }))
        .expect("parses");
        assert_eq!(body.initiative_ids, [initiative_id]);

        serde_json::from_value::<RequestBody>(json!({
            "initiativeIds": [initiative_id],
            "change": { "kind": "delete" },
            "force": true,
        }))
        .expect_err("unknown fields are refused");
    }
}
