// Copyright © 2026 Jalapeno Labs

//! The company doc: its fixed sections, the suggestions waiting on them, and the review's place on
//! the timeline. See `docs/knowledge.md`, The company doc.
//!
//! One document per instance knows the company, in seven fixed [`Section`]s of Markdown. Each
//! section is a page (`crate::models::page`), proposal-only and fixed in its place under the
//! Company page, so people edit it as they edit any page, and everyone else suggests an edit to it
//! through the framework every page shares (`crate::models::page_suggestion`), which the doc adds
//! no rule to. A change to the doc is a new version of the section it touches, indexed as its own
//! document in the same transaction, so search finds what the doc says the moment it commits, and
//! the doc's history is its sections' pages'.
//!
//! The review's position on the timeline is kept here too ([`review_cursor`],
//! [`advance_review_cursor`]); what it reads and how it suggests is `crate::company_doc::review`.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::{Uuid, uuid};

use crate::database::schema::{company_doc_review_cursor, pages};
use crate::models::page_suggestion::{self, Suggestion, SuggestionState};

/// The longest a section may be, in characters, as `pages_company_doc_section_fits` allows. Seven
/// of them fit the review's one prompt beside what it reads (`crate::company_doc::review`).
pub const CONTENT_MAX_CHARACTERS: usize = 12_000;

/// One of the doc's fixed sections, in the order the doc is read.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Section {
    /// Who the company is: what it does, for whom, and where.
    Company,
    /// Who leads it, and what each of them owns.
    Leadership,
    /// Everyone who comes up, with how their name is spelled and what they do.
    People,
    /// Clients, partners, investors, and vendors, and who speaks for each.
    Stakeholders,
    /// What the company makes and sells.
    Products,
    /// The work under way, and who is on it.
    Projects,
    /// Names, acronyms, and jargon, spelled as the company spells them.
    Glossary,
}

impl Section {
    /// Every section, in the order the doc is read.
    pub const ALL: [Self; 7] = [
        Self::Company,
        Self::Leadership,
        Self::People,
        Self::Stakeholders,
        Self::Products,
        Self::Projects,
        Self::Glossary,
    ];

    /// The page that holds it, seeded by its migration at this id.
    pub const fn page_id(self) -> Uuid {
        match self {
            Self::Company => uuid!("00000000-0000-0000-0002-000000000001"),
            Self::Leadership => uuid!("00000000-0000-0000-0002-000000000002"),
            Self::People => uuid!("00000000-0000-0000-0002-000000000003"),
            Self::Stakeholders => uuid!("00000000-0000-0000-0002-000000000004"),
            Self::Products => uuid!("00000000-0000-0000-0002-000000000005"),
            Self::Projects => uuid!("00000000-0000-0000-0002-000000000006"),
            Self::Glossary => uuid!("00000000-0000-0000-0002-000000000007"),
        }
    }

    /// The section page `id` holds, if it holds one.
    pub fn of_page(id: Uuid) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|section| section.page_id() == id)
    }

    /// The key sent to clients and agents.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Company => "company",
            Self::Leadership => "leadership",
            Self::People => "people",
            Self::Stakeholders => "stakeholders",
            Self::Products => "products",
            Self::Projects => "projects",
            Self::Glossary => "glossary",
        }
    }

    /// The section keyed `key`, if it is one.
    pub fn parse(key: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|section| section.as_str() == key)
    }

    /// Its heading, as the index titles its document and prompts name it. English, since only
    /// people's own words are translated in the web app, and it names the section by its key.
    pub const fn heading(self) -> &'static str {
        match self {
            Self::Company => "Company",
            Self::Leadership => "Leadership",
            Self::People => "People",
            Self::Stakeholders => "Stakeholders",
            Self::Products => "Products",
            Self::Projects => "Projects",
            Self::Glossary => "Glossary",
        }
    }

    /// What belongs there, for the agents that suggest to it.
    pub const fn purpose(self) -> &'static str {
        match self {
            Self::Company => "Who the company is: what it does, for whom, and where.",
            Self::Leadership => "Who leads the company, and what each of them owns.",
            Self::People => {
                "Everyone who comes up, one per line: their name exactly as it is spelled, how \
                 it is often misheard or misspelled, and what they do."
            }
            Self::Stakeholders => {
                "Clients, partners, investors, and vendors, one per line, with who speaks for \
                 each."
            }
            Self::Products => "What the company makes and sells, one per line.",
            Self::Projects => "The work under way, one per line, with who is on it.",
            Self::Glossary => {
                "Names, acronyms, and jargon, one per line in alphabetical order: the term as \
                 the company spells it, what it means, and how it is often misheard."
            }
        }
    }
}

/// A section as it stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocSection {
    pub section: Section,
    pub content: String,
    pub updated_at: DateTime<Utc>,
}

/// A suggestion to one of the doc's sections.
#[derive(Debug, Clone)]
pub struct SectionSuggestion {
    pub section: Section,
    pub suggestion: Suggestion,
}

/// The ids of the pages that hold the sections.
fn section_pages() -> [Uuid; Section::ALL.len()] {
    Section::ALL.map(Section::page_id)
}

/// Every section, in order.
///
/// # Errors
/// Propagates any database error.
pub async fn sections(connection: &mut AsyncPgConnection) -> QueryResult<Vec<DocSection>> {
    let rows: Vec<(Uuid, String, DateTime<Utc>)> = pages::table
        .filter(pages::id.eq_any(section_pages()))
        .select((pages::id, pages::body, pages::updated_at))
        .load(connection)
        .await?;
    let mut sections: Vec<DocSection> = rows
        .into_iter()
        .filter_map(|(id, content, updated_at)| {
            Some(DocSection {
                section: Section::of_page(id)?,
                content,
                updated_at,
            })
        })
        .collect();
    sections.sort_by_key(|section| section.section);
    Ok(sections)
}

/// Every suggestion waiting on the doc's sections, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn pending(connection: &mut AsyncPgConnection) -> QueryResult<Vec<SectionSuggestion>> {
    let pending = page_suggestion::pending_on(connection, &section_pages()).await?;
    Ok(by_section(pending))
}

/// The newest `newest` suggestions to the doc's sections in `state`, oldest first, as the review
/// reads what waits and what people dismissed.
///
/// # Errors
/// Propagates any database error.
pub async fn latest(
    connection: &mut AsyncPgConnection,
    state: SuggestionState,
    newest: i64,
) -> QueryResult<Vec<SectionSuggestion>> {
    let latest = page_suggestion::latest_on(connection, &section_pages(), state, newest).await?;
    Ok(by_section(latest))
}

/// `suggestions`, each with the section its page holds. Every one is on a section's page, since
/// each was read by those pages' ids.
fn by_section(suggestions: Vec<Suggestion>) -> Vec<SectionSuggestion> {
    suggestions
        .into_iter()
        .filter_map(|suggestion| {
            Some(SectionSuggestion {
                section: Section::of_page(suggestion.page_id)?,
                suggestion,
            })
        })
        .collect()
}

/// How far the review has read the timeline: the ingestion instant and id of the last record it
/// read, or `None` before its first review.
///
/// # Errors
/// Propagates any database error.
pub async fn review_cursor(
    connection: &mut AsyncPgConnection,
) -> QueryResult<Option<(DateTime<Utc>, Uuid)>> {
    let (at, id): (Option<DateTime<Utc>>, Option<Uuid>) = company_doc_review_cursor::table
        .select((
            company_doc_review_cursor::reviewed_through_at,
            company_doc_review_cursor::reviewed_through_id,
        ))
        .first(connection)
        .await?;
    Ok(at.zip(id))
}

/// Moves the review's position from `from` to `to`, unless another review moved it first, which
/// answers `false` and moves nothing. Part of the caller's transaction, which stores what the
/// review suggested from the records up to `to`, so both happen or neither does.
///
/// # Errors
/// Propagates any database error.
pub async fn advance_review_cursor(
    connection: &mut AsyncPgConnection,
    from: Option<(DateTime<Utc>, Uuid)>,
    to: (DateTime<Utc>, Uuid),
) -> QueryResult<bool> {
    let current = company_doc_review_cursor::table
        .select((
            company_doc_review_cursor::reviewed_through_at,
            company_doc_review_cursor::reviewed_through_id,
        ))
        .for_update()
        .first::<(Option<DateTime<Utc>>, Option<Uuid>)>(connection)
        .await?;
    if current.0.zip(current.1) != from {
        return Ok(false);
    }
    diesel::update(company_doc_review_cursor::table)
        .set((
            company_doc_review_cursor::reviewed_through_at.eq(to.0),
            company_doc_review_cursor::reviewed_through_id.eq(to.1),
        ))
        .execute(connection)
        .await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn every_section_round_trips_through_its_key_and_its_page() {
        for section in Section::ALL {
            assert_eq!(Section::parse(section.as_str()), Some(section));
            assert_eq!(Section::of_page(section.page_id()), Some(section));
            assert_eq!(
                serde_json::to_value(section).expect("serializes"),
                json!(section.as_str())
            );
        }
        assert_eq!(Section::parse("Glossary"), None, "keys are lowercase");
    }
}
