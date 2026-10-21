// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/elysia/messages`: a person writes to Elysia. The message joins the
//! conversation and queues a turn for her to answer it, which starts as soon as she is free,
//! whatever her hours say.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use diesel_async::AsyncConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{MessageResponse, TurnResponse, publish_snapshot};
use crate::auth::CurrentUser;
use crate::elysia::trigger;
use crate::errors::ApiError;
use crate::models::elysia::{self, NewTurn};
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::validate_not_blank;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    #[validate(length(min = 1, max = 20000), custom(function = "validate_not_blank"))]
    body: String,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let author = current.id();
    let (message, turn) = connection
        .transaction(async move |connection| {
            let message = elysia::insert_message(connection, &body.body, author).await?;
            let turn = elysia::insert_turn(
                connection,
                NewTurn {
                    id: Uuid::now_v7(),
                    trigger_kind: trigger::MESSAGE.to_owned(),
                    trigger_detail: json!({ "messageId": message.id }),
                    created_by: author,
                },
            )
            .await?;
            diesel::QueryResult::Ok((message, turn))
        })
        .await?;

    let message = MessageResponse::from(message);
    state
        .events
        .publish(&ServerEvent::ElysiaMessageCreated(message.clone()));
    state
        .events
        .publish(&ServerEvent::ElysiaTurnUpserted(TurnResponse::from(turn)));
    publish_snapshot(&state.events, &mut connection).await?;
    state.elysia.wake();

    Ok((StatusCode::CREATED, Json(json!({ "message": message }))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_needs_a_body_that_is_not_blank() {
        for refused in [json!({ "body": "" }), json!({ "body": "  \n" })] {
            let body: RequestBody = serde_json::from_value(refused).expect("parses");
            body.validate().expect_err("refused");
        }
        let body: RequestBody =
            serde_json::from_value(json!({ "body": "Draft the quarterly deck." })).expect("parses");
        body.validate().expect("valid");
    }
}
