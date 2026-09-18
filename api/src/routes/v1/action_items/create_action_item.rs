// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items`: the user adds an item by hand.
//!
//! The user created it, so it starts `open` rather than in the inbox.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{
    publish_item_write, refuse_unknown_projects, unique_ids, validate_not_blank, validate_owner,
};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item::{self, ActionItemPriority, ActionItemState, NewActionItem, Owner};
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 500), custom(function = "validate_not_blank"))]
    title: String,
    #[serde(default)]
    #[validate(length(max = 20000))]
    notes: String,
    #[serde(default = "normal_priority")]
    priority: ActionItemPriority,
    due_at: Option<DateTime<Utc>>,
    #[serde(default = "the_user")]
    #[validate(custom(function = "validate_owner"))]
    owner: Owner,
    #[serde(default)]
    project_ids: Vec<Uuid>,
    #[serde(default)]
    initiative_ids: Vec<Uuid>,
}

const fn normal_priority() -> ActionItemPriority {
    ActionItemPriority::Normal
}

const fn the_user() -> Owner {
    Owner::User
}

pub async fn handle(
    State(state): State<AppState>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let new_item = NewActionItem {
        title: body.title,
        notes: body.notes,
        state: ActionItemState::Open,
        priority: body.priority,
        due_at: body.due_at,
        owner: body.owner,
        project_ids: unique_ids(body.project_ids),
        initiative_ids: unique_ids(body.initiative_ids),
    };
    let now = Utc::now();

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let created = action_item::create(&mut connection, new_item, Actor::User, now)
        .await
        .map_err(refuse_unknown_projects)?;
    let item = publish_item_write(&state, &mut connection, created, &[], now).await?;

    Ok((StatusCode::CREATED, Json(json!({ "item": item }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: Value) -> Result<RequestBody, serde_json::Error> {
        serde_json::from_value(body)
    }

    #[test]
    fn a_title_alone_makes_a_normal_item_for_the_user() {
        let body = parse(json!({ "title": "Reply to Sam" })).expect("parses");

        body.validate().expect("valid");
        assert_eq!(body.priority, ActionItemPriority::Normal);
        assert_eq!(body.owner, Owner::User);
        assert!(body.project_ids.is_empty() && body.initiative_ids.is_empty());
    }

    #[test]
    fn blank_or_oversized_titles_are_refused() {
        for title in ["   ".to_owned(), "x".repeat(501)] {
            let body = parse(json!({ "title": title })).expect("parses");
            assert!(
                body.validate()
                    .expect_err("refused")
                    .field_errors()
                    .contains_key("title")
            );
        }
    }

    #[test]
    fn due_dates_need_an_offset_and_priorities_must_be_known() {
        parse(json!({ "title": "A", "dueAt": "2026-10-01T09:00:00" }))
            .expect_err("a timestamp without an offset is ambiguous");
        let offset = parse(json!({ "title": "A", "dueAt": "2026-10-01T09:00:00+02:00" }))
            .expect("offset timestamps parse");
        assert_eq!(
            offset.due_at.expect("set").to_rfc3339(),
            "2026-10-01T07:00:00+00:00"
        );
        parse(json!({ "title": "A", "priority": "critical" })).expect_err("unknown priority");
        parse(json!({ "title": "A", "state": "resolved" }))
            .expect_err("new items always start open");
    }
}
