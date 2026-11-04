// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/company-doc/review`: review what was ingested since the last review now, rather
//! than at the schedule's next turn. The review has one live run at a time, so while one is
//! queued, running, or waiting on a person, this answers that run instead of starting another.
//! Answers the run's id and state, for the Runs page.

use anyhow::Context;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use chrono::Utc;
use serde_json::{Value, json};

use crate::auth::MemberUser;
use crate::company_doc::workflow;
use crate::errors::ApiError;
use crate::state::AppState;
use crate::workflows;

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let started = workflows::start(
        &mut connection,
        &state.events,
        workflow::run_for(Utc::now(), member.id()),
    )
    .await?;
    let status = if started.created {
        StatusCode::CREATED
    } else {
        StatusCode::OK
    };

    Ok((
        status,
        Json(json!({
            "run": { "id": started.settled.run.id, "state": started.settled.run.state },
            "started": started.created,
        })),
    ))
}
