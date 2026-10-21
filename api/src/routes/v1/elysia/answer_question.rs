// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/elysia/questions/{id}/answer`: a person answers one of Elysia's questions,
//! with one of her suggestions or in their own words. The answer queues a turn, so she
//! continues the work she asked about.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use diesel_async::AsyncConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{QuestionResponse, TurnResponse, publish_snapshot};
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
    #[validate(length(min = 1, max = 2000), custom(function = "validate_not_blank"))]
    answer: String,
}

pub async fn handle(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let person = current.id();
    let answered = connection
        .transaction(async move |connection| {
            let Some(question) =
                elysia::close_question(connection, id, Some(&body.answer), person).await?
            else {
                return diesel::QueryResult::Ok(None);
            };
            let turn = elysia::insert_turn(
                connection,
                NewTurn {
                    id: Uuid::now_v7(),
                    trigger_kind: trigger::ANSWER.to_owned(),
                    trigger_detail: json!({ "questionId": question.id }),
                    created_by: person,
                },
            )
            .await?;
            Ok(Some((question, turn)))
        })
        .await?;
    let Some((question, turn)) = answered else {
        return Err(ApiError::CodedConflict {
            code: "question_not_open",
            message: "the question was already answered or dismissed",
        });
    };

    state
        .events
        .publish(&ServerEvent::ElysiaTurnUpserted(TurnResponse::from(turn)));
    publish_snapshot(&state.events, &mut connection).await?;
    state.elysia.wake();

    Ok(Json(
        json!({ "question": QuestionResponse::from(question) }),
    ))
}
