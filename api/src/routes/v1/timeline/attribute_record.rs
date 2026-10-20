// Copyright © 2026 Jalapeno Labs

//! `PUT` and `DELETE /api/v1/timeline/{id}/projects/{projectId}`: the record's owner says
//! which projects it is about. A person's word is certain, so their attribution has a
//! confidence of 1 and replaces whatever Elysia guessed for that project.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde_json::{Value, json};
use uuid::Uuid;

use super::TimelineRecordResponse;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::timeline_record;
use crate::state::AppState;

/// How sure a person's own attribution is.
const PERSON_CONFIDENCE: f32 = 1.0;

pub async fn attribute(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (record, _attributions) =
        timeline_record::find_owned(&mut connection, id, current.id()).await?;

    let attributed = timeline_record::attribute(
        &mut connection,
        record.id,
        project_id,
        PERSON_CONFIDENCE,
        &current.actor().to_string(),
    )
    .await;
    match attributed {
        Ok(_attribution) => {}
        Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
            return Err(ApiError::BadRequest(
                "projectId names no project".to_owned(),
            ));
        }
        Err(error) => return Err(error.into()),
    }

    let (record, attributions) =
        timeline_record::find_owned(&mut connection, id, current.id()).await?;
    Ok(Json(
        json!({ "record": TimelineRecordResponse::new(record, attributions) }),
    ))
}

pub async fn unattribute(
    State(state): State<AppState>,
    current: CurrentUser,
    path: Result<Path<(Uuid, Uuid)>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path((id, project_id)) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let (record, _attributions) =
        timeline_record::find_owned(&mut connection, id, current.id()).await?;
    timeline_record::unattribute(&mut connection, record.id, project_id).await?;

    let (record, attributions) =
        timeline_record::find_owned(&mut connection, id, current.id()).await?;
    Ok(Json(
        json!({ "record": TimelineRecordResponse::new(record, attributions) }),
    ))
}
