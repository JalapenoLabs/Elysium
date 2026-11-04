// Copyright © 2026 Jalapeno Labs

//! `GET /api/v1/knowledge/documents/{id}?fromPosition=&toPosition=`: one document of the
//! knowledge index, as a citation opens it, whoever's record it came from. Whole, or the span of
//! its passages between two positions, which a search answers beside each passage it found.
//!
//! The work is [`open`], which the MCP server's `knowledge_get` and the agents' tool of the same
//! name call too, with [`DocumentArguments`] as their arguments.

use anyhow::Context;
use axum::Json;
use axum::extract::rejection::{PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
use validator::{Validate, ValidationError};

use crate::auth::MemberUser;
use crate::database::Pool;
use crate::errors::ApiError;
use crate::models::knowledge_document::{self, DocumentView};
use crate::state::AppState;

/// Which of a document's passages to answer: every one, or those between two positions.
#[derive(Debug, Default, Deserialize, Validate)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
#[validate(schema(function = "validate_span"))]
pub struct PassageSpan {
    #[validate(range(min = 0))]
    from_position: Option<i32>,
    #[validate(range(min = 0))]
    to_position: Option<i32>,
}

impl PassageSpan {
    /// Whether the passage at `position` is inside the span. Both ends are inclusive.
    fn holds(&self, position: i32) -> bool {
        let after_start = self.from_position.is_none_or(|from| position >= from);
        let before_end = self.to_position.is_none_or(|to| position <= to);
        after_start && before_end
    }
}

/// A span that ends before it starts holds nothing, which is a mistake to say rather than answer
/// as a document without passages.
fn validate_span(span: &PassageSpan) -> Result<(), ValidationError> {
    match (span.from_position, span.to_position) {
        (Some(from), Some(to)) if from > to => Err(ValidationError::new("span")
            .with_message("`fromPosition` must not be after `toPosition`".into())),
        _ => Ok(()),
    }
}

/// One document of the knowledge base, as a tool names it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DocumentArguments {
    /// The document's id: a citation's `documentId`, from a search or a meeting's topics.
    pub document_id: Uuid,
    /// The first passage to answer, by its position, which a search gives beside each passage
    /// it found. From the first passage when left out.
    pub from_position: Option<i32>,
    /// The last passage to answer, by its position. To the last passage when left out.
    pub to_position: Option<i32>,
}

impl DocumentArguments {
    /// The document and the span of it these arguments name.
    pub const fn into_parts(self) -> (Uuid, PassageSpan) {
        let span = PassageSpan {
            from_position: self.from_position,
            to_position: self.to_position,
        };
        (self.document_id, span)
    }
}

pub async fn handle(
    State(state): State<AppState>,
    _member: MemberUser,
    path: Result<Path<Uuid>, PathRejection>,
    span: Result<Query<PassageSpan>, QueryRejection>,
) -> Result<Json<Value>, ApiError> {
    let Path(id) = path?;
    let Query(span) = span?;
    let document = open(&state.database, id, &span).await?;
    Ok(Json(json!({ "document": document })))
}

/// The document `id`, with the passages `span` names, in order.
///
/// It answers any document: callers check that whoever asks may read the knowledge base
/// (`docs/knowledge.md`, Who sees what). `passageCount` still counts every passage, so a reader
/// of a span knows how much more there is.
///
/// # Errors
/// [`ApiError::Validation`] for a span that ends before it starts, [`ApiError::NotFound`] when
/// the index holds no such document, and an internal error when the database does not answer.
pub async fn open(database: &Pool, id: Uuid, span: &PassageSpan) -> Result<DocumentView, ApiError> {
    span.validate()?;
    let mut connection = database
        .get()
        .await
        .context("no database connection available")?;
    let mut document = knowledge_document::get(&mut connection, id)
        .await?
        .ok_or(ApiError::NotFound)?;
    document
        .passages
        .retain(|passage| span.holds(passage.position));
    Ok(document)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_span_holds_the_passages_between_its_ends_inclusive() {
        let whole = PassageSpan::default();
        assert!(whole.holds(0) && whole.holds(62));

        let middle = PassageSpan {
            from_position: Some(2),
            to_position: Some(4),
        };
        let held: Vec<i32> = (0..7).filter(|position| middle.holds(*position)).collect();
        assert_eq!(held, [2, 3, 4]);

        let tail = PassageSpan {
            from_position: Some(5),
            to_position: None,
        };
        assert!(!tail.holds(4) && tail.holds(5) && tail.holds(500));
    }

    #[test]
    fn a_span_that_ends_before_it_starts_is_refused() {
        let backwards = PassageSpan {
            from_position: Some(3),
            to_position: Some(2),
        };
        backwards.validate().expect_err("backwards");

        let one = PassageSpan {
            from_position: Some(3),
            to_position: Some(3),
        };
        one.validate().expect("a span of one passage is a span");

        let negative = PassageSpan {
            from_position: Some(-1),
            to_position: None,
        };
        negative.validate().expect_err("positions start at 0");
    }

    #[test]
    fn a_tools_arguments_name_the_document_and_its_span() {
        let id = Uuid::now_v7();
        let arguments: DocumentArguments = serde_json::from_value(json!({
            "documentId": id,
            "fromPosition": 1,
        }))
        .expect("parses");
        let (document_id, span) = arguments.into_parts();
        assert_eq!(document_id, id);
        assert!(!span.holds(0) && span.holds(1));

        serde_json::from_value::<DocumentArguments>(json!({ "id": id }))
            .expect_err("the document is named documentId");
    }
}
