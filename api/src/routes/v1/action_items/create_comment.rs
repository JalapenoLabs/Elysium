// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/{id}/comments`: the user comments on an item.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{CommentResponse, publish_history, validate_not_blank};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_comment;
use crate::models::action_item_event::Recorded;
use crate::realtime::ServerEvent;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 20000), custom(function = "validate_not_blank"))]
    pub body: String,
}

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let Recorded { record, history } =
        action_item_comment::create(&mut connection, id, body.body, Actor::User, Utc::now())
            .await?;
    drop(connection);

    let comment = CommentResponse::from(record);
    publish_history(&state, history);
    state
        .events
        .publish(&ServerEvent::ActionItemCommentUpserted(comment.clone()));

    Ok((StatusCode::CREATED, Json(json!({ "comment": comment }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_need_a_body_that_is_not_blank() {
        for refused in [json!({ "body": "" }), json!({ "body": " \n " })] {
            let body: RequestBody = serde_json::from_value(refused).expect("parses");
            body.validate().expect_err("refused");
        }
        let body: RequestBody =
            serde_json::from_value(json!({ "body": "Asked Sam for the logs." })).expect("parses");
        body.validate().expect("valid");
    }
}
