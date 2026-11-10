// Copyright © 2026 Jalapeno Labs

//! `/api/v1/projects/{id}/autonomy`: the project's trust dial, what of its Project Manager's work
//! waits for a person (`crate::models::project_autonomy`, `docs/autonomy.md`, The dial).
//!
//! `GET` answers its level with who set it and when (`setBy` and `setAt`, both `null` while it is
//! at the default it started with). `PUT { autonomy }` sets it, recorded with the person who did,
//! and takes a member: no machine user is one, so neither Elysia nor a Project Manager turns it,
//! and a guest cannot. It applies to what the Project Manager submits and changes from then on. The
//! change is announced as `project.upserted`, whose project carries the level, and
//! `projectManager.changed`, since the Project Manager's tab says how its changes reach the work.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::ProjectResponse;
use crate::auth::{CurrentUser, MemberUser};
use crate::errors::ApiError;
use crate::models::project;
use crate::models::project_autonomy::{self, Autonomy};
use crate::realtime::ServerEvent;
use crate::state::AppState;

/// A project's level, as its page shows it.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutonomyResponse {
    autonomy: Autonomy,
    /// The person who set it, or `null` at the default the project started with.
    set_by: Option<Uuid>,
    set_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    autonomy: Autonomy,
}

pub async fn get(
    State(state): State<AppState>,
    _current: CurrentUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let autonomy = project::find(&mut connection, id).await?.autonomy;
    let response = respond(&mut connection, id, autonomy).await?;
    Ok(Json(json!({ "autonomy": response })))
}

pub async fn put(
    State(state): State<AppState>,
    person: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let project = project_autonomy::set(&mut connection, id, body.autonomy, &person).await?;
    let response = respond(&mut connection, id, project.autonomy).await?;

    state
        .events
        .publish(&ServerEvent::ProjectUpserted(ProjectResponse::from(
            project.clone(),
        )));
    state
        .events
        .publish(&ServerEvent::ProjectManagerChanged { project_id: id });

    Ok(Json(json!({
        "project": ProjectResponse::from(project),
        "autonomy": response,
    })))
}

/// `project_id`'s level `autonomy` with who set it and when.
async fn respond(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    autonomy: Autonomy,
) -> Result<AutonomyResponse, ApiError> {
    let change = project_autonomy::latest_change(connection, project_id).await?;
    Ok(AutonomyResponse {
        autonomy,
        set_by: change.as_ref().map(|change| change.created_by),
        set_at: change.map(|change| change.created_at),
    })
}

#[cfg(test)]
mod tests;
