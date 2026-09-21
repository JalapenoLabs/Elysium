// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/projects/{id}`: delete a project that no session belongs to.
//!
//! Deleting a project never takes its sessions with it. Each session owns a thread on
//! a satellite, and those are destroyed deliberately, one session at a time.
//!
//! Its action items and initiatives stay, taken out of the project in the same
//! transaction, each with the removal in its history and on the event stream.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::AsyncConnection;
use uuid::Uuid;

use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_event::Recorded;
use crate::models::{action_item, initiative, project};
use crate::realtime::ServerEvent;
use crate::routes::v1::action_items::{item_responses, publish_history};
use crate::routes::v1::initiatives::publish_initiative_write;
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;

    let now = Utc::now();
    let deleted = connection
        .transaction(async move |connection| {
            project::lock_for_delete(connection, id).await?;
            let items =
                action_item::remove_all_from_project(connection, id, Actor::User, now).await?;
            let initiatives =
                initiative::remove_all_from_project(connection, id, Actor::User, now).await?;
            project::delete(connection, id).await?;
            Ok::<_, DieselError>((items, initiatives))
        })
        .await;
    let (items, initiatives) = match deleted {
        Ok(removed) => removed,
        Err(DieselError::DatabaseError(DatabaseErrorKind::ForeignKeyViolation, _)) => {
            return Err(ApiError::Conflict(
                "the project still has coding sessions; delete them first",
            ));
        }
        Err(other) => return Err(other.into()),
    };

    // The deletion is the fact clients must not miss, so it goes out before anything that
    // could fail. Leaving a project moves no initiative's progress, so items are published
    // on their own, once each, with no initiative fan-out.
    state.events.publish(&ServerEvent::ProjectDeleted { id });

    let mut live_items = Vec::new();
    for Recorded { record, history } in items {
        publish_history(&state.events, history);
        if record.deleted_at.is_none() {
            live_items.push(record);
        }
    }
    for item in item_responses(&mut connection, live_items).await? {
        state.events.publish(&ServerEvent::ActionItemUpserted(item));
    }
    for initiative in initiatives {
        publish_initiative_write(&state.events, &mut connection, initiative, now).await?;
    }

    Ok(StatusCode::NO_CONTENT)
}
