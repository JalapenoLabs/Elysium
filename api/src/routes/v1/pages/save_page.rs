// Copyright © 2026 Jalapeno Labs

//! What an agent's `page_save` does, for the MCP server and a turn's `elysium_knowledge` alike.
//!
//! An agent has one tool for writing a page where the browser has two routes, so it takes one flat
//! set of arguments, and this tells them apart and calls what each route calls: [`create`] for a new
//! page, and [`publish`] over an existing one. Everything those do still happens: the version and
//! history naming the author, the refusals, the index, and the announcement.
//!
//! A learning stays out of it, unlike in the browser, where a person edits one as any page. An
//! agent saves a learning with `learning_save`, which revises by title and takes its evidence with
//! what it says, so a rule is never rewritten over the evidence for the rule it replaced; a turn
//! has no way to save one yet (`docs/knowledge.md`, MCP). Refusing also keeps a new page placed
//! under Learnings from passing for a learning: it would be an ordinary page, never listed as one.

use anyhow::Context;
use schemars::JsonSchema;
use serde::Deserialize;
use uuid::Uuid;

use super::{
    CreatePageRequest, PublishOutcome, PublishPageRequest, PublishedPage, create, publish,
};
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::agent::Author;
use crate::models::page::{self, LEARNINGS_PAGE_ID};
use crate::realtime::EventBus;

/// The `code` of an agent's write that belongs to `learning_save`.
pub const PAGE_IS_A_LEARNING: &str = "page_is_a_learning";

/// What refusing one says.
const LEARNING_REFUSAL: &str = "a learning is saved with learning_save, which keeps its evidence \
    with what it says, so page_save leaves learnings alone";

/// A new page or a page's next version. MCP clients take one flat object, so which it is shows in
/// what is given, and [`save`] refuses a mixture by name.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavePageRequest {
    /// The page to change, from `page_list` or `page_read`. Leave it out to create a page.
    #[serde(default)]
    pub id: Option<Uuid>,
    /// With `id`: the `currentVersion` `page_read` answered, the version this was edited from.
    /// Saving over a newer one is refused, so read the page again and make the change on what it
    /// says now.
    #[serde(default)]
    pub base_version: Option<i32>,
    /// Without `id`: the page it goes under, or `null` for the top of the tree. Required when
    /// creating, even when `null`, so nothing lands at the top of the tree by accident. Moving a
    /// page that exists is `page_move`.
    #[serde(
        default,
        deserialize_with = "::serde_with::rust::double_option::deserialize"
    )]
    #[expect(
        clippy::option_option,
        reason = "left out, null, and a page are three distinct requests"
    )]
    pub parent_id: Option<Option<Uuid>>,
    /// Its title, on one line of up to 300 characters.
    pub title: String,
    /// What it says, in Markdown, up to 100,000 characters: the page whole, not a change to it. It
    /// replaces what is there.
    pub body: String,
}

/// Saves `request` as `author`, creating a page or publishing its next version.
///
/// # Errors
/// `400` for arguments that are neither a new page nor a version of one, `409 page_is_a_learning`
/// for a learning or a new page placed directly under Learnings, and whatever [`create`] and
/// [`publish`] refuse.
pub async fn save(
    database: &Pool,
    events: &EventBus,
    author: Author<'_>,
    request: SavePageRequest,
) -> Result<PublishedPage, ApiError> {
    let SavePageRequest {
        id,
        base_version,
        parent_id,
        title,
        body,
    } = request;
    let learning_refused = ApiError::CodedConflict {
        code: PAGE_IS_A_LEARNING,
        message: LEARNING_REFUSAL,
    };
    match (id, base_version, parent_id) {
        (Some(id), Some(base_version), None) => {
            if is_learning(database, id).await? {
                return Err(learning_refused);
            }
            let version = PublishPageRequest {
                base_version,
                title,
                body,
                category: None,
                evidence: None,
            };
            publish(database, events, author, id, version).await
        }
        (None, None, Some(parent_id)) => {
            if parent_id == Some(LEARNINGS_PAGE_ID) {
                return Err(learning_refused);
            }
            let new_page = CreatePageRequest {
                parent_id,
                title,
                body,
            };
            let page = create(database, events, author, new_page).await?;
            Ok(PublishedPage {
                page,
                outcome: PublishOutcome::Created,
            })
        }
        (Some(_), None, _) => Err(ApiError::BadRequest(
            "changing a page takes baseVersion, the currentVersion page_read answered".to_owned(),
        )),
        (Some(_), Some(_), Some(_)) => Err(ApiError::BadRequest(
            "page_save changes what a page says; page_move changes where it sits".to_owned(),
        )),
        (None, Some(_), _) => Err(ApiError::BadRequest(
            "baseVersion names a version of a page: give that page's id, or leave baseVersion out \
             to create a page"
                .to_owned(),
        )),
        (None, None, None) => Err(ApiError::BadRequest(
            "creating a page takes parentId, the page it goes under from page_list, or null for \
             the top of the tree"
                .to_owned(),
        )),
    }
}

/// Whether page `id` is a learning. A page never becomes one or stops being one, so nothing can
/// change this between here and the publish.
async fn is_learning(database: &Pool, id: Uuid) -> Result<bool, ApiError> {
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    Ok(page::find(&mut connection, id).await?.is_learning())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_parent_of_null_is_told_apart_from_no_parent_at_all() {
        let top: SavePageRequest = serde_json::from_value(json!({
            "parentId": null,
            "title": "Runbooks",
            "body": "",
        }))
        .expect("parses");
        assert_eq!(top.parent_id, Some(None), "the top of the tree, on purpose");

        let forgotten: SavePageRequest =
            serde_json::from_value(json!({ "title": "Runbooks", "body": "" })).expect("parses");
        assert_eq!(forgotten.parent_id, None, "said nothing about where");

        let parent = Uuid::now_v7();
        let under: SavePageRequest = serde_json::from_value(json!({
            "parentId": parent,
            "title": "Runbooks",
            "body": "",
        }))
        .expect("parses");
        assert_eq!(under.parent_id, Some(Some(parent)));
    }

    #[test]
    fn the_body_is_always_given_so_a_forgotten_one_never_blanks_a_page() {
        serde_json::from_value::<SavePageRequest>(json!({
            "id": Uuid::now_v7(),
            "baseVersion": 2,
            "title": "Runbooks",
        }))
        .expect_err("the body is required");
    }

    #[test]
    fn a_learnings_fields_are_not_arguments() {
        serde_json::from_value::<SavePageRequest>(json!({
            "id": Uuid::now_v7(),
            "baseVersion": 2,
            "title": "Lint first",
            "body": "Run the linter.",
            "category": "convention",
        }))
        .expect_err("learning_save takes the category");
    }
}
