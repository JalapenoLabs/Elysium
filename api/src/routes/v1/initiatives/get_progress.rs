// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/initiatives/{id}/progress`: resolved and total now, and a burnup of both
//! from the initiative's creation to now.
//!
//! `burnup` holds a point at creation, one at every moment either count changed, and one
//! now, so clients draw it as steps. The counting rules are
//! `crate::action_items::progress`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::action_items::progress;
use crate::errors::ApiError;
use crate::models::initiative;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let found = initiative::find(&mut connection, id).await?;
    let spans = initiative::memberships(&mut connection, &[id])
        .await?
        .remove(&id)
        .unwrap_or_default();
    let current = progress::at(&spans, now);
    let burnup = progress::burnup(&spans, found.created_at, now);

    Ok(Json(json!({
        "resolved": current.resolved,
        "total": current.total,
        "burnup": burnup,
    })))
}
