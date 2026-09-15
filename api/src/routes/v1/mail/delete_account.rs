// Copyright © 2026 Jalapeno Labs

//! `DELETE /api/v1/mail/accounts/{id}`: disconnect a mailbox.
//!
//! A self-hosted mailbox is destroyed on the mail server first, mail included, so its
//! address is free again. An OAuth account is only forgotten: the mailbox belongs to
//! its owner, who can also revoke Elysium's access from their provider's settings.

use anyhow::Context;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use uuid::Uuid;

use super::require_administrator;
use crate::errors::ApiError;
use crate::models::mail_account::{self, MailAccountKind};
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
    let account = mail_account::find(&mut connection, id).await?;
    drop(connection);

    if account.kind == MailAccountKind::SelfHosted {
        let administrator = require_administrator(
            &state,
            "the mail server does not exist, so the mailbox cannot be removed from it",
        )
        .await?;
        let stalwart_id = account
            .external_id
            .as_deref()
            .context("a self-hosted mailbox row has no Stalwart id")?;
        state
            .mail
            .stalwart
            .destroy_mailbox(&administrator, stalwart_id)
            .await?;
    }

    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    mail_account::delete(&mut connection, id).await?;

    state
        .events
        .publish(&ServerEvent::MailAccountDeleted { id });
    Ok(StatusCode::NO_CONTENT)
}
