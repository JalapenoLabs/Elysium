// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/action-items/{id}/links/remote`: what each of an item's linked things looks
//! like in its provider right now, read live.
//!
//! An item holds none of a provider's fields, so the item page shows them from here beside
//! the item's own. Every link is read at once, and one that cannot be read answers its own
//! error rather than failing the rest.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use futures_util::future::join_all;
use serde_json::{Value, json};
use tracing::{Level, event};
use uuid::Uuid;

use crate::action_items::links::LinkError;
use crate::errors::ApiError;
use crate::models::{action_item, action_item_link};
use crate::state::AppState;

pub async fn handle(
    State(state): State<AppState>,
    path: Result<Path<Uuid>, PathRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    action_item::find(&mut connection, id).await?;
    let links = action_item_link::list_for_item(&mut connection, id).await?;
    drop(connection);

    let reads = links.iter().map(|link| async {
        let read = state.links.provider(link.provider).read(link).await;
        match read {
            Ok(remote) => json!({ "linkId": link.id, "remote": remote, "error": null }),
            Err(LinkError::Internal(error)) => {
                event!(
                    name: "links.remote.read.failed",
                    Level::ERROR,
                    link.id = %link.id,
                    error.message = %error,
                    error.chain = ?error,
                    "a link could not be read inside Elysium",
                );
                json!({
                    "linkId": link.id,
                    "remote": null,
                    "error": "Elysium could not read it; its logs say why",
                })
            }
            Err(error) => json!({ "linkId": link.id, "remote": null, "error": error.to_string() }),
        }
    });
    let remotes = join_all(reads).await;

    Ok(Json(json!({ "remotes": remotes })))
}
