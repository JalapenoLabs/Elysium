// Copyright © 2026 Jalapeno Labs

//! `PUT` and `DELETE /api/v1/timeline/{id}/projects/{projectId}`: the record's owner says
//! which projects it is about. A person's word is certain, so their attribution has a
//! confidence of 1 and replaces whatever Elysia guessed for that project, and Elysia never
//! changes it back (`crate::models::attribution`).
//!
//! Each change is recorded in the record's history, and the answer names that entry
//! (`entryId`, `null` when nothing changed), which the web app's Undo reverses.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use serde_json::Value;
use uuid::Uuid;

use super::answer_changed;
use crate::auth::CurrentUser;
use crate::errors::ApiError;
use crate::models::attribution::{self, Attributor};
use crate::models::timeline_record;
use crate::state::AppState;

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

    let written = attribution::attribute(
        &mut connection,
        record.id,
        project_id,
        attribution::PERSON_CONFIDENCE,
        Attributor::Person(current.id()),
    )
    .await;
    let written = match written {
        Ok(written) => written,
        Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
            return Err(ApiError::BadRequest(
                "projectId names no project".to_owned(),
            ));
        }
        Err(error) => return Err(error.into()),
    };
    answer_changed(&state, &mut connection, id, current.id(), written.entry()).await
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
    let written = attribution::unattribute(
        &mut connection,
        record.id,
        project_id,
        Attributor::Person(current.id()),
    )
    .await?;
    answer_changed(&state, &mut connection, id, current.id(), written.entry()).await
}
