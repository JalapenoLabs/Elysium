// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/mail/domains/{id}`: stop hosting a domain.
//!
//! Refused while any mailbox lives on the domain, and for the server's default domain,
//! the one it was created with, which Stalwart keeps for itself.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::require_administrator;
use crate::errors::ApiError;
use crate::models::mail_domain;
use crate::realtime::ServerEvent;
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
    let domain = mail_domain::find(&mut connection, id).await?;
    if domain.is_default {
        return Err(ApiError::Conflict(
            "the server's first domain is its default and cannot be removed",
        ));
    }
    if mail_domain::mailbox_count(&mut connection, id).await? > 0 {
        return Err(ApiError::Conflict(
            "the domain still has mailboxes; delete them first",
        ));
    }
    drop(connection);

    let administrator = require_administrator(
        &state,
        "the mail server does not exist, so the domain cannot be removed from it",
    )
    .await?;
    state
        .mail
        .stalwart
        .destroy_domain(&administrator, &domain.stalwart_id)
        .await?;

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    mail_domain::delete(&mut connection, id).await?;

    state.events.publish(&ServerEvent::MailDomainDeleted { id });
    Ok(StatusCode::NO_CONTENT)
}
