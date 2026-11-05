// Copyright © 2026 Jalapeno Labs

//! `POST /api/v1/pages/{id}/move`: move a page, with everything under it, or reorder it among its
//! siblings. The move is recorded in its history; a page already there records nothing.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{PageResponse, answer, author};
use crate::auth::MemberUser;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page::{self, Placement};
use crate::state::AppState;

/// Where a page goes. Both are required, even when `null`, so leaving one out never sends a page to
/// the top of the tree or the top of its siblings by accident.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MovePageRequest {
    /// The page it goes under, or `null` for the top of the tree.
    #[serde(deserialize_with = "Option::deserialize")]
    pub parent_id: Option<Uuid>,
    /// The sibling it goes right after, or `null` to go first.
    #[serde(deserialize_with = "Option::deserialize")]
    pub after: Option<Uuid>,
}

pub async fn handle(
    State(state): State<AppState>,
    member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    body: Result<Json<MovePageRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Json(body) = body?;
    let page = move_to(&state, author(&member), id, body).await?;
    Ok(Json(json!({ "page": page })))
}

/// Moves page `id` to `request` as `author`: what this route and an agent's tool both do.
///
/// # Errors
/// `404` for an unknown page, and `409` with `page_deleted`, `page_proposal_only`,
/// `page_parent_gone`, `page_under_itself`, or `page_placement_gone` for a move that is refused.
pub async fn move_to(
    state: &AppState,
    author: Author<'_>,
    id: Uuid,
    request: MovePageRequest,
) -> Result<PageResponse, ApiError> {
    let placement = Placement {
        parent_id: request.parent_id,
        after: request.after,
    };
    let mut connection = state
        .database
        .get()
        .await
        .context("no database connection available")?;
    let recorded = page::move_page(&mut connection, id, placement, author).await?;
    answer(state, &mut connection, recorded).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_move_says_where_on_purpose() {
        let top: MovePageRequest =
            serde_json::from_value(json!({ "parentId": null, "after": null })).expect("parses");
        assert_eq!(top.parent_id, None);
        assert_eq!(top.after, None);

        serde_json::from_value::<MovePageRequest>(json!({ "after": null }))
            .expect_err("the parent is required");
        serde_json::from_value::<MovePageRequest>(json!({ "parentId": null }))
            .expect_err("the sibling is required");
    }
}
