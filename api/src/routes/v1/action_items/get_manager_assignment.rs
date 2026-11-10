// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/action-items/{id}/manager-assignment`: where an item's plan stands while it is
//! assigned to its lead project's Project Manager: who assigned it and when, whether its Project
//! Manager has planned it yet, and the batches naming it (`crate::models::action_item_assignment`).
//! `null` while it is not assigned.
//!
//! Batches are for the company's members, as the work queue is.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::action_item;
use crate::models::action_item_assignment::{self, Plan, PlanBatch, PlanStage};
use crate::state::AppState;

/// An item's plan as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PlanResponse {
    /// The lead project, whose Project Manager it is assigned to.
    project_id: Uuid,
    assigned_at: DateTime<Utc>,
    assigned_by: Uuid,
    stage: PlanStage,
    /// The turn that took the assignment, once one did, which its project's page opens.
    turn_id: Option<Uuid>,
    batches: Vec<PlanBatch>,
}

impl From<Plan> for PlanResponse {
    fn from(plan: Plan) -> Self {
        Self {
            project_id: plan.project_id,
            assigned_at: plan.assigned_at,
            assigned_by: plan.assigned_by,
            stage: plan.stage,
            turn_id: plan.turn_id,
            batches: plan.batches,
        }
    }
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let item = action_item::find(&mut connection, id).await?;
    let plan = action_item_assignment::plan(&mut connection, &item)
        .await?
        .map(PlanResponse::from);

    Ok(Json(json!({ "plan": plan })))
}
