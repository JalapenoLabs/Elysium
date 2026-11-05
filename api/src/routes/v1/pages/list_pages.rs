// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/pages`: every live page without its text, in order among its siblings, which a
//! client builds the tree from by `parentId`; or with `?deleted=true` every deleted one, most
//! recently deleted first.
//!
//! An agent reads the same list a branch at a time ([`list_branch`]): every learning is a page, so
//! the whole tree soon outgrows what a tool may answer, and it needs to see how the tree is laid
//! out, not all of it at once. Learnings all sit under one page, so a single branch can outgrow it
//! too, and each answer stops at [`BRANCH_LIMIT`].

use std::collections::HashMap;

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use super::PageSummaryResponse;
use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::{page, page_label};
use crate::state::AppState;

/// The most pages one `page_list` answers, as `LIST_DESCRIPTION` says.
///
/// Claude Code refuses a tool's result past about 25,000 tokens, some 300 pages of a branch, and a
/// page's labels and a long title take up more than the average. Lowering it only costs an agent
/// another call.
const BRANCH_LIMIT: usize = 200;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListQuery {
    #[serde(default)]
    deleted: bool,
}

/// The branch of the tree `page_list` reads.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ListPagesArguments {
    /// List the pages directly under this one. Leave it out for the top of the tree.
    #[serde(default)]
    pub parent_id: Option<Uuid>,
    /// How many of the branch's pages to skip, for the rest of a branch longer than one answer.
    #[serde(default)]
    pub offset: usize,
}

/// A page of a branch, with how far the tree goes on under it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchPage {
    #[serde(flatten)]
    page: PageSummaryResponse,
    /// How many live pages sit directly under it.
    child_count: usize,
}

/// What `page_list` answers: a stretch of one branch, and how long the branch is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Branch {
    pages: Vec<BranchPage>,
    /// How many live pages the branch holds, however many this answer carries.
    total: usize,
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Query(query) = query?;
    let pages = list(&state.database, query.deleted).await?;
    Ok(Json(json!({ "pages": pages })))
}

/// The tree's live pages, or the deleted ones: what this route and an agent's tool both read.
///
/// # Errors
/// Any database error.
pub async fn list(database: &Pool, deleted: bool) -> Result<Vec<PageSummaryResponse>, ApiError> {
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let mut labels = page_label::by_page(&mut connection).await?;
    let pages = page::list(&mut connection, deleted)
        .await?
        .into_iter()
        .map(|page| {
            let labels = labels.remove(&page.id).unwrap_or_default();
            PageSummaryResponse::new(page, labels)
        })
        .collect();
    Ok(pages)
}

/// The live pages directly under `parent_id`, or at the top of the tree, from what this route reads:
/// what an agent's tool lists, at most [`BRANCH_LIMIT`] of them from `offset` on.
///
/// # Errors
/// `404` for a parent that does not exist or is deleted, and any database error.
pub async fn list_branch(
    database: &Pool,
    parent_id: Option<Uuid>,
    offset: usize,
) -> Result<Branch, ApiError> {
    let pages = list(database, false).await?;
    branch_of(pages, parent_id, offset).ok_or(ApiError::NotFound)
}

/// The stretch of `tree` directly under `parent_id` that starts at `offset`, in the order given, or
/// `None` when no page of `tree` is `parent_id`.
fn branch_of(
    tree: Vec<PageSummaryResponse>,
    parent_id: Option<Uuid>,
    offset: usize,
) -> Option<Branch> {
    let mut parent_found = parent_id.is_none();
    let mut child_counts: HashMap<Uuid, usize> = HashMap::new();
    for page in &tree {
        parent_found |= Some(page.id) == parent_id;
        if let Some(parent) = page.parent_id {
            *child_counts.entry(parent).or_default() += 1;
        }
    }
    if !parent_found {
        return None;
    }
    let branch: Vec<PageSummaryResponse> = tree
        .into_iter()
        .filter(|page| page.parent_id == parent_id)
        .collect();
    let total = branch.len();
    let pages = branch
        .into_iter()
        .skip(offset)
        .take(BRANCH_LIMIT)
        .map(|page| BranchPage {
            child_count: child_counts.get(&page.id).copied().unwrap_or_default(),
            page,
        })
        .collect();
    Some(Branch { pages, total })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn summary(id: Uuid, parent_id: Option<Uuid>, position: i64) -> PageSummaryResponse {
        PageSummaryResponse {
            id,
            parent_id,
            position,
            title: "A page".to_owned(),
            current_version: 1,
            fixed: false,
            labels: Vec::new(),
            updated_at: Utc::now(),
            deleted_at: None,
        }
    }

    #[test]
    fn a_branch_is_one_level_of_the_tree_in_order_counting_what_is_under_each_page() {
        let (top, other, child, grandchild) = (
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
            Uuid::now_v7(),
        );
        let tree = || {
            vec![
                summary(other, None, 1),
                summary(child, Some(top), 1),
                summary(top, None, 2),
                summary(grandchild, Some(child), 1),
            ]
        };

        let at_the_top: Vec<(Uuid, usize)> = branch_of(tree(), None, 0)
            .expect("the top of the tree is always there")
            .pages
            .into_iter()
            .map(|entry| (entry.page.id, entry.child_count))
            .collect();
        assert_eq!(at_the_top, [(other, 0), (top, 1)]);

        let under_the_top: Vec<(Uuid, usize)> = branch_of(tree(), Some(top), 0)
            .expect("the parent is there")
            .pages
            .into_iter()
            .map(|entry| (entry.page.id, entry.child_count))
            .collect();
        assert_eq!(
            under_the_top,
            [(child, 1)],
            "only what is directly under it"
        );

        let leaf = branch_of(tree(), Some(grandchild), 0).expect("the parent is there");
        assert!(
            leaf.pages.is_empty(),
            "a page with nothing under it lists nothing"
        );
    }

    #[test]
    fn a_parent_not_on_the_tree_is_no_branch_at_all() {
        let tree = vec![summary(Uuid::now_v7(), None, 1)];
        assert!(branch_of(tree, Some(Uuid::now_v7()), 0).is_none());
    }

    #[test]
    fn a_long_branch_comes_a_stretch_at_a_time_without_losing_a_page() {
        // Learnings all sit under one page, and a branch past what a tool may answer would be
        // refused whole.
        let learnings = Uuid::now_v7();
        let mut tree = vec![summary(learnings, None, 1)];
        let total = BRANCH_LIMIT + 5;
        let children: Vec<Uuid> = (0..total).map(|_| Uuid::now_v7()).collect();
        for (position, child) in (1_i64..).zip(&children) {
            tree.push(summary(*child, Some(learnings), position));
        }

        let first = branch_of(tree.clone(), Some(learnings), 0).expect("the parent is there");
        assert_eq!(first.pages.len(), BRANCH_LIMIT);
        assert_eq!(first.total, total);

        let rest = branch_of(tree.clone(), Some(learnings), BRANCH_LIMIT).expect("still there");
        assert_eq!(
            rest.total, total,
            "the total is the branch's, not the answer's"
        );
        let listed: Vec<Uuid> = first
            .pages
            .iter()
            .chain(&rest.pages)
            .map(|entry| entry.page.id)
            .collect();
        assert_eq!(listed, children, "every page once, in order");

        let past_the_end = branch_of(tree, Some(learnings), total).expect("still there");
        assert!(past_the_end.pages.is_empty());
    }

    #[test]
    fn a_branch_page_reads_as_a_summary_with_its_child_count() {
        let id = Uuid::now_v7();
        let entry = BranchPage {
            page: summary(id, None, 1),
            child_count: 3,
        };
        let read = serde_json::to_value(entry).expect("serializes");
        assert_eq!(read["id"], json!(id));
        assert_eq!(read["childCount"], 3);
        assert_eq!(
            read["labels"],
            json!([]),
            "with the labels a summary carries"
        );
    }
}
