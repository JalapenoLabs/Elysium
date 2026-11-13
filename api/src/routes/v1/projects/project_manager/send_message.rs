// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/projects/{id}/project-manager/messages`: a person writes to the project's
//! Project Manager. The message wakes it at once, taking whatever else waits with it; its answer
//! shows under the message on its turn.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::ProjectManagerResponse;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::meeting::Reader;
use crate::models::project;
use crate::project_managers;
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
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(project_id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    project::find(&mut connection, project_id).await?;
    project_managers::message(
        &mut connection,
        &state.events,
        project_id,
        body.body.trim(),
        member.id(),
    )
    .await?;
    let snapshot =
        project_managers::snapshot(&mut connection, project_id, Reader::of(&member.0)).await?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "projectManager": ProjectManagerResponse::from(snapshot) })),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_needs_a_body_that_is_not_blank() {
        for refused in [json!({ "body": "" }), json!({ "body": " \n" })] {
            let body: RequestBody = serde_json::from_value(refused).expect("parses");
            body.validate().expect_err("refused");
        }
        let body: RequestBody =
            serde_json::from_value(json!({ "body": "Prioritize the pricing page." }))
                .expect("parses");
        body.validate().expect("valid");
    }
}
