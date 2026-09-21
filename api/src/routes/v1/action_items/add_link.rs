// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/action-items/{id}/links`: link an item to a Jira issue, a GitHub issue, or
//! a GitHub pull request that a picker listed.
//!
//! The thing is read through its credential first, so a link is never made to anything the
//! credential cannot reach or that is not what `kind` says. The item's first link becomes
//! its primary, and the item's owner then follows the thing's assignee. Linking the same
//! thing again answers the link unchanged; one already linked to another item answers `409`.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::Validate;

use super::{LinkTarget, link_responses, publish_item_links, publish_item_write};
use crate::action_items::Actor;
use crate::errors::ApiError;
use crate::models::action_item_link::{self, NewLink};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<LinkTarget>, JsonRejection>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let Path(id) = path?;
    let Json(target) = body?;
    target.validate()?;

    let remote = state
        .links
        .provider(target.provider)
        .find(target.credential_id, target.kind, &target.reference)
        .await?;
    let new_link = NewLink {
        credential: target.credential(),
        kind: target.kind,
        external_id: remote.external_id.clone(),
        observation: remote.observation(),
    };

    let now = Utc::now();
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let credential = new_link.credential;
    let linked =
        action_item_link::add(&mut connection, id, new_link, true, Actor::User, now).await?;
    publish_item_write(&state.events, &mut connection, linked.item, &[], now).await?;
    publish_item_links(&state.events, &mut connection, id).await?;

    let link = action_item_link::find_by_external(
        &mut connection,
        credential,
        target.kind,
        &remote.external_id,
    )
    .await?
    .context("the link just made can be read back")?;
    let link = link_responses(&mut connection, vec![link])
        .await?
        .pop()
        .context("one link in, one response out")?;

    Ok((StatusCode::CREATED, Json(json!({ "link": link }))))
}
