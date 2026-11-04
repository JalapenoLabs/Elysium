// Copyright © 2026 Jalapeno Labs

//! `/api/v1/learning-categories`: the categories learnings are filed under, Elysium's nine
//! built-in ones and the workspace's own (`docs/knowledge.md`, Categories).
//!
//! The company's members read the list, since they file learnings under it; a guest reaches no
//! knowledge base. Only admins add, edit, reorder, and remove the workspace's own categories,
//! which are a workspace setting. The MCP server's `learning_save` reads the same list to tell
//! agents what each category holds (`crate::mcp::learnings`).

mod create_category;
mod list_categories;
mod remove_category;
#[cfg(test)]
mod tests;
mod update_category;

use axum::Router;
use axum::routing::{get, patch};
use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;
use validator::ValidationError;

use crate::models::learning_category::LearningCategory;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route(
            "/",
            get(list_categories::handle).post(create_category::handle),
        )
        .route(
            "/{key}",
            patch(update_category::handle).delete(remove_category::handle),
        )
}

/// The most characters a key holds, as `learning_categories.key` allows.
const KEY_MAX_CHARACTERS: usize = 50;

/// A category as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LearningCategoryResponse {
    key: String,
    label: String,
    description: String,
    /// Its place among the categories of its kind; the list is already in order.
    position: i32,
    /// One of Elysium's nine, which cannot be changed or removed.
    built_in: bool,
    /// How many learnings are filed under it, deleted ones included: what removing it moves.
    learning_count: i64,
    created_by: Uuid,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl LearningCategoryResponse {
    fn new(category: LearningCategory, learning_count: i64) -> Self {
        Self {
            key: category.key,
            label: category.label,
            description: category.description,
            position: category.position,
            built_in: category.built_in,
            learning_count,
            created_by: category.created_by,
            created_at: category.created_at,
            updated_at: category.updated_at,
        }
    }
}

/// A key a learning can store and an agent can type: a lowercase letter, then lowercase
/// letters, digits, and underscores, such as `eda_tools`. The rule `learning_categories.key`
/// checks.
fn validate_key(key: &str) -> Result<(), ValidationError> {
    let mut characters = key.chars();
    let starts_with_a_letter = characters
        .next()
        .is_some_and(|first| first.is_ascii_lowercase());
    let rest_is_plain = characters.all(|character| {
        character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_'
    });
    if starts_with_a_letter && rest_is_plain && key.len() <= KEY_MAX_CHARACTERS {
        return Ok(());
    }
    Err(ValidationError::new("key").with_message(
        "a key is a lowercase letter, then up to 49 lowercase letters, digits, or underscores"
            .into(),
    ))
}

#[cfg(test)]
mod key_tests {
    use super::*;

    #[test]
    fn a_key_is_lowercase_words_joined_by_underscores() {
        for key in [
            "rtl",
            "eda_tools",
            "skills_and_agents",
            "soc2",
            &"k".repeat(50),
        ] {
            validate_key(key).unwrap_or_else(|_| panic!("{key} is a key"));
        }
        for key in [
            "",
            "RTL",
            "eda-tools",
            "2fa",
            "_private",
            "has space",
            "café",
            &"k".repeat(51),
        ] {
            validate_key(key).expect_err(key);
        }
    }
}
