// Copyright © 2026 Jalapeno Labs

//! `PUT /api/v1/projects/{id}/project-manager/tracker`: name the tracker the project's Project
//! Manager files issues in, overriding the precedent of the issues its items already link.
//!
//! The place is read through the credential that would file there before anything is stored, so
//! a project never names a Jira project outside its credential's allowlist, a repository its
//! token cannot see, or a YouTrack project its account does not reach. It is stored as the
//! provider names it.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{ProjectManagerResponse, tracker_credential};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::action_item_link::LinkProvider;
use crate::models::meeting::Reader;
use crate::models::project;
use crate::models::project_issue_tracker::{self, NewTracker};
use crate::project_managers;
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::validate_not_blank;
use crate::state::AppState;

#[derive(Debug, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestBody {
    provider: LinkProvider,
    /// The Jira credential or YouTrack account that files there; absent for GitHub.
    credential_id: Option<Uuid>,
    /// A Jira project key, a GitHub `owner/name`, or a YouTrack project's short name, as the
    /// destinations listing names it.
    #[validate(length(min = 1, max = 300), custom(function = "validate_not_blank"))]
    destination: String,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(project_id) = path?;
    let Json(body) = body?;
    body.validate()?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    project::find(&mut connection, project_id).await?;
    let credential_id = tracker_credential(
        &mut connection,
        project_id,
        body.provider,
        body.credential_id,
        member.id(),
    )
    .await?;
    // Reading a provider can take seconds; no connection is held while it does.
    drop(connection);
    let destination = state
        .links
        .provider(body.provider)
        .destination(credential_id, &body.destination)
        .await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let columns = crate::models::action_item_link::LinkCredential {
        provider: body.provider,
        id: credential_id,
    }
    .columns();
    project_issue_tracker::set(
        &mut connection,
        &NewTracker {
            project_id,
            provider: body.provider,
            jira_credential_id: columns.jira_credential_id,
            token_account_id: columns.token_account_id,
            destination: destination.reference,
            created_by: member.id(),
        },
    )
    .await?;
    state
        .events
        .publish(&ServerEvent::ProjectManagerChanged { project_id });

    let snapshot =
        project_managers::snapshot(&mut connection, project_id, Reader::of(&member.0)).await?;
    Ok(Json(json!({
        "projectManager": ProjectManagerResponse::from(snapshot),
    })))
}
