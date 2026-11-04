// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/projects/{id}/project-manager/destinations?provider=&credentialId=`: the places a
//! tracker on `provider` could file this project's issues, read through the credential that
//! would file there, for the tracker picker. A GitHub listing names no credential: it lists what
//! the project's own token reaches.

use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::tracker_credential;
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::action_item_link::LinkProvider;
use crate::models::project;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Parameters {
    provider: LinkProvider,
    credential_id: Option<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    query: Result<Query<Parameters>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(project_id) = path?;
    let Query(parameters) = query?;

    let mut connection = state.database.get().await.map_err(anyhow::Error::from)?;
    project::find(&mut connection, project_id).await?;
    let credential_id = tracker_credential(
        &mut connection,
        project_id,
        parameters.provider,
        parameters.credential_id,
        member.id(),
    )
    .await?;
    // Listing a provider can take seconds; no connection is held while it does.
    drop(connection);
    let destinations = state
        .links
        .provider(parameters.provider)
        .destinations(credential_id)
        .await?;
    Ok(Json(json!({ "destinations": destinations })))
}
