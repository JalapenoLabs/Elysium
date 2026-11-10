// Copyright © 2026 Jalapeno Labs

//! `/api/v1/findings`: a person deciding on a finding a worker or a Project Manager reported,
//! by accepting or dismissing it; a project's open findings are `GET
//! /api/v1/projects/{id}/findings`. A finding quotes agents' work on the project, so every route
//! answers the company's members alone, and only those members decide, since no machine user is
//! one. See `docs/engine.md`, Findings, and `crate::findings`.

mod accept_finding;
mod dismiss_finding;
pub mod list_findings;
#[cfg(test)]
mod tests;

use axum::Router;
use axum::routing::post;
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::models::finding::{Cost, Finding, FindingState};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/{id}/accept", post(accept_finding::handle))
        .route("/{id}/dismiss", post(dismiss_finding::handle))
}

/// One finding, whole.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FindingResponse {
    id: Uuid,
    project_id: Uuid,
    title: String,
    detail: String,
    repository_url: Option<String>,
    location: String,
    cost: Cost,
    state: FindingState,
    /// The batch whose worker reported it first, or the Project Manager's turn that did.
    work_batch_id: Option<Uuid>,
    project_manager_turn_id: Option<Uuid>,
    /// How many times it was reported.
    reports: i32,
    last_reported_at: DateTime<Utc>,
    decided_by: Option<Uuid>,
    decided_at: Option<DateTime<Utc>>,
    dismissal_reason: Option<String>,
    /// What accepting it made.
    action_item_id: Option<Uuid>,
    accepted_batch_id: Option<Uuid>,
    /// The machine user of whoever reported it first.
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

impl From<Finding> for FindingResponse {
    fn from(finding: Finding) -> Self {
        Self {
            id: finding.id,
            project_id: finding.project_id,
            title: finding.title,
            detail: finding.detail,
            repository_url: finding.repository_url,
            location: finding.location,
            cost: finding.cost,
            state: finding.state,
            work_batch_id: finding.work_batch_id,
            project_manager_turn_id: finding.project_manager_turn_id,
            reports: finding.reports,
            last_reported_at: finding.last_reported_at,
            decided_by: finding.decided_by,
            decided_at: finding.decided_at,
            dismissal_reason: finding.dismissal_reason,
            action_item_id: finding.action_item_id,
            accepted_batch_id: finding.accepted_batch_id,
            created_by: finding.created_by,
            created_at: finding.created_at,
        }
    }
}
