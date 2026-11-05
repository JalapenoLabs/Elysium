// Copyright © 2026 Jalapeno Labs

//! Labels on knowledge base pages: any page takes any of them, and they cut across the tree.
//!
//! A label is not content, so writing one makes no version: [`add`] and [`remove`] change
//! `page_labels` and record an entry in the page's history, under the lock on its row every write
//! of a page takes. Nothing about a label is copied into the knowledge index. The search's filter
//! and a document's own labels read `page_labels` through the document's page, as the project filter
//! reads a record's attributions, so a change is in the index the moment it commits and cannot touch
//! a passage or its embedding (`docs/knowledge.md`, Pages, Labels).
//!
//! Who may label follows from that. A page agents only propose to keeps what it says from them, and
//! a fixed page keeps its place and name; neither is what a label changes, so both take labels from
//! people and agents alike, and the history says who. Only a deleted page refuses, since it changes
//! no further.

use std::collections::HashMap;
use std::fmt;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::json;
use uuid::Uuid;
use validator::{ValidationError, ValidationErrors};

use crate::database::schema::page_labels;
use crate::models::agent::Author;
use crate::models::page::{self, HistoryKind, Page, PageError, Recorded};

/// The longest a label may be, in characters, as `page_labels_label_length` allows: room for a
/// few words, and short enough to read as a chip.
const MAX_LABEL_CHARACTERS: usize = 50;

/// The most labels one page carries. Nothing about labels needs a limit this low to work; it only
/// bounds what a page shows and what an agent's loop can pile on, and no page reads well past it.
pub const MAX_LABELS_PER_PAGE: i64 = 50;

/// A label as it is stored and compared.
///
/// Two people typing `On Call` and `on-call` mean one label, so it is lowercase with runs of spaces
/// made one hyphen, whichever way it is written or searched for. It starts with a letter or digit
/// and holds only those, `-`, `_`, and `.`, so it is one segment of a path however it is encoded: a
/// label of dots or holding a slash would otherwise rewrite the address it is removed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label(String);

impl Label {
    /// The label `text` stands for.
    ///
    /// # Errors
    /// What is wrong with it, keyed `label`, so it reads as any refused field does.
    pub fn parse(text: &str) -> Result<Self, ValidationErrors> {
        let label = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join("-")
            .to_lowercase();

        let mut characters = label.chars();
        let starts_well = characters.next().is_some_and(char::is_alphanumeric);
        let holds_only_label_characters = characters
            .all(|character| character.is_alphanumeric() || matches!(character, '-' | '_' | '.'));
        if starts_well && holds_only_label_characters {
            let length = label.chars().count();
            if length <= MAX_LABEL_CHARACTERS {
                return Ok(Self(label));
            }
        }

        let mut errors = ValidationErrors::new();
        errors.add(
            "label",
            ValidationError::new("label").with_message(
                format!(
                    "a label is at most {MAX_LABEL_CHARACTERS} characters: letters, digits, \
                     hyphens, underscores, and dots, starting with a letter or digit"
                )
                .into(),
            ),
        );
        Err(errors)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Insertable)]
#[diesel(table_name = page_labels)]
struct LabelRow<'a> {
    page_id: Uuid,
    label: &'a str,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

/// Refuses a change to `page`'s labels while it is deleted. This is all that refuses one: see the
/// module's documentation for why a proposal-only or fixed page takes them.
const fn require_live(page: &Page) -> Result<(), PageError> {
    if page.deleted_at.is_some() {
        return Err(PageError::Deleted);
    }
    Ok(())
}

/// Puts `label` on page `id` and records it in the page's history. A label already there records
/// nothing.
///
/// # Errors
/// `NotFound` for an unknown page, [`PageError::Deleted`] for a deleted one, and
/// [`PageError::TooManyLabels`] for a page already carrying [`MAX_LABELS_PER_PAGE`].
pub async fn add(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    label: &Label,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = page::lock(connection, id).await?;
            require_live(&page)?;
            let added = diesel::insert_into(page_labels::table)
                .values(LabelRow {
                    page_id: id,
                    label: label.as_str(),
                    created_by: author.user_id,
                    created_at: author.at,
                })
                .on_conflict_do_nothing()
                .execute(connection)
                .await?;
            if added == 0 {
                return Ok(Recorded::unchanged(page));
            }

            // Counted after the insert, so a page at the limit still answers for a label it has,
            // and exact, since every change to a page's labels holds its row. Refusing rolls the
            // insert back.
            let carried: i64 = page_labels::table
                .filter(page_labels::page_id.eq(id))
                .count()
                .get_result(connection)
                .await?;
            if carried > MAX_LABELS_PER_PAGE {
                return Err(PageError::TooManyLabels {
                    max: MAX_LABELS_PER_PAGE,
                });
            }

            let data = json!({ "label": label.as_str() });
            let entry =
                page::record(connection, id, None, HistoryKind::Labeled, author, data).await?;
            Ok(Recorded::of(page, entry))
        })
        .await
}

/// Takes `label` off page `id` and records it in the page's history. A label the page does not
/// carry records nothing.
///
/// # Errors
/// `NotFound` for an unknown page and [`PageError::Deleted`] for a deleted one.
pub async fn remove(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    label: &Label,
    author: Author<'_>,
) -> Result<Recorded, PageError> {
    connection
        .transaction(async move |connection| {
            let page = page::lock(connection, id).await?;
            require_live(&page)?;
            let removed = diesel::delete(page_labels::table.find((id, label.as_str())))
                .execute(connection)
                .await?;
            if removed == 0 {
                return Ok(Recorded::unchanged(page));
            }

            let data = json!({ "label": label.as_str() });
            let entry =
                page::record(connection, id, None, HistoryKind::Unlabeled, author, data).await?;
            Ok(Recorded::of(page, entry))
        })
        .await
}

/// The labels page `id` carries, in order.
///
/// # Errors
/// Propagates any database error.
pub async fn of_page(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<Vec<String>> {
    page_labels::table
        .filter(page_labels::page_id.eq(id))
        .order(page_labels::label.asc())
        .select(page_labels::label)
        .load(connection)
        .await
}

/// Every page's labels, in order, by page, in one read for the tree. A page carrying none has no
/// entry.
///
/// # Errors
/// Propagates any database error.
pub async fn by_page(
    connection: &mut AsyncPgConnection,
) -> QueryResult<HashMap<Uuid, Vec<String>>> {
    let rows: Vec<(Uuid, String)> = page_labels::table
        .order((page_labels::page_id.asc(), page_labels::label.asc()))
        .select((page_labels::page_id, page_labels::label))
        .load(connection)
        .await?;

    let mut labels: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (page_id, label) in rows {
        labels.entry(page_id).or_default().push(label);
    }
    Ok(labels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_label_reads_the_same_however_it_was_typed() {
        for typed in ["On Call", "  on   call ", "ON-CALL", "on-call", "on\tcall"] {
            assert_eq!(Label::parse(typed).expect("parses").as_str(), "on-call");
        }
    }

    #[test]
    fn a_label_keeps_every_alphabet_and_its_punctuation() {
        for label in ["v1.2", "q3_2026", "日本語", "büro", "2026"] {
            assert_eq!(Label::parse(label).expect("parses").as_str(), label);
        }
    }

    #[test]
    fn a_label_that_would_rewrite_its_address_is_refused() {
        for bad in ["..", ".", ".hidden", "a/b", "a\\b", "a?b", "a#b", "a%2Fb"] {
            assert!(Label::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn an_empty_label_or_one_past_the_limit_is_refused_and_says_why() {
        for bad in ["", "   ", "-leading", "a,b", "nul\0"] {
            assert!(Label::parse(bad).is_err(), "{bad:?}");
        }

        let longest = "x".repeat(MAX_LABEL_CHARACTERS);
        assert_eq!(Label::parse(&longest).expect("fits").as_str(), longest);

        let errors = Label::parse(&format!("{longest}x")).expect_err("too long");
        let field = errors.field_errors();
        let message = field["label"][0].message.as_deref();
        assert!(message.is_some_and(|message| message.contains("at most 50 characters")));
    }

    #[test]
    fn the_limit_counts_characters_not_bytes() {
        let longest = "語".repeat(MAX_LABEL_CHARACTERS);
        Label::parse(&longest).expect("fits");
    }
}
