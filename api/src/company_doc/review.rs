// Copyright © 2026 Jalapeno Labs

//! The review's one turn: the prompt the model reads, and the checks its answer has to pass.
//! Pure; the step that runs it is `super::workflow`.
//!
//! The review is a text turn on the runner (`crate::runner::TurnMode::Text`): no tools, no
//! settings, only the doc, the edits already suggested to it, and what was ingested since the
//! last review, and a JSON answer back. Nothing the answer says is taken on trust: [`parse_answer`]
//! keeps a proposal only for a section the doc has, with edits and a reason, at most one per
//! section, and cites a record only when it was one offered. Each kept proposal is then suggested
//! like any agent's edit (`crate::models::page_suggestion::suggest`), whose edits must apply to
//! the section as it stands. The answer's JSON names them proposals, as the prompt asks.
//!
//! # Size
//!
//! One review reads a batch of what was ingested, not all of it at once: six hours of a busy
//! company is more than one turn weighs well, and every turn costs the instance's shared Claude
//! usage. The doc, at most 12,000 characters a section, goes whole, since the model edits what it
//! reads; suggestions go as compact diffs, each cut short; and records fill what is left of
//! [`PROMPT_BUDGET_BYTES`], each body cut to [`RECORD_BODY_CHARACTERS`], oldest first, so the
//! records that did not fit are read by the next turn.

use std::collections::HashSet;
use std::fmt::Write as _;

use serde::Deserialize;
use uuid::Uuid;

use crate::models::company_doc::{DocSection, Section, SectionSuggestion};
use crate::models::page_suggestion::Edit;
use crate::models::timeline_record::TimelineRecord;

/// The most bytes one prompt carries: the doc whole and a batch of records beside it, about
/// 30,000 tokens, which keeps each review's turn quick and its cost known.
pub const PROMPT_BUDGET_BYTES: usize = 120_000;

/// The least room records must have for a turn to be worth running. A doc that leaves less is
/// one the review cannot read beside anything new; its sections need shortening.
const MIN_RECORDS_BYTES: usize = 8_000;

/// How much of a record's body the prompt shows: a message whole, a long page or thread's
/// opening, where names and terms are introduced.
pub const RECORD_BODY_CHARACTERS: usize = 1_500;

/// How much of each suggestion's diff the prompt shows.
const PROPOSAL_DIFF_CHARACTERS: usize = 1_500;

/// The most suggestions the prompt lists, pending and dismissed each, the newest kept.
pub const PROPOSALS_SHOWN: i64 = 30;

/// The longest source kept for a cited record, as `page_suggestions.source` allows.
const SOURCE_CHARACTERS: usize = 500;

/// The longest reason and quote kept, as `page_suggestions` allows.
const REASON_CHARACTERS: usize = 2_000;
const QUOTE_CHARACTERS: usize = 2_000;

/// The most edits a proposal holds, as an agent's suggestion may.
const EDITS_MAX: usize = crate::models::page_suggestion::EDITS_MAX;

pub const SYSTEM_PROMPT: &str = "\
You keep a company's reference doc current. It has fixed sections: company, leadership, people, \
stakeholders, products, projects, and glossary. Transcripts of the company's meetings are \
corrected against it, so the spelling of names and jargon matters most: spell a person's name as \
they write it themselves, in their own messages and signatures, over how a transcript heard it. \
You are given the doc, the changes already proposed and waiting for review, the changes people \
dismissed and why, and messages, meetings, pages, and issues ingested since the last review. \
Propose only what those records show: a new person, stakeholder, product, project, or term; a \
corrected spelling; a changed role or owner. Never propose what a waiting proposal already says, \
or what people dismissed. Keep each section's style; list sections hold one entry per line, and the \
glossary is in alphabetical order. \
Answer with one JSON object and nothing else: \
{\"proposals\": [{\"section\": string, \"edits\": [{\"find\": string, \"replace\": string}], \
\"reason\": string, \"quote\": string, \"recordId\": string}]}. \
At most one proposal per section. An edit's find is text the section holds exactly once, copied \
exactly, which replace takes the place of; an empty find adds replace at the end of the section. \
The reason is one or two sentences a person reviewing it reads first. The quote is the words in \
the record that show it, exactly as written, and recordId is that record's id. Answer \
{\"proposals\": []} when nothing in the records belongs in the doc.";

/// The review's prompt, and how many of the records it was given fit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub text: String,
    /// The first this many records, in the order given, are in the prompt.
    pub records_included: usize,
}

/// Why no prompt could be built.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PromptError {
    #[error(
        "the company doc is too long for a review to read it beside anything new; shorten its \
         longest sections"
    )]
    DocTooLong,
    #[error("there are no records to review")]
    NoRecords,
}

/// The prompt for one turn over `records`, oldest first, beside the doc's `sections` and the
/// `pending` and `dismissed` suggestions made to it.
///
/// # Errors
/// [`PromptError::DocTooLong`] when the doc and its suggestions leave too little room for records,
/// and [`PromptError::NoRecords`] when even the first record does not fit or there is none.
pub fn prompt(
    sections: &[DocSection],
    pending: &[SectionSuggestion],
    dismissed: &[SectionSuggestion],
    records: &[TimelineRecord],
) -> Result<Prompt, PromptError> {
    let mut text = String::from("The company doc, section by section:\n");
    for section in sections {
        let _ = writeln!(
            text,
            "\n## {} (section key: {})\nWhat belongs here: {}",
            section.section.heading(),
            section.section.as_str(),
            section.section.purpose()
        );
        if section.content.trim().is_empty() {
            text.push_str("(empty)\n");
        } else {
            let _ = writeln!(text, "{}", section.content);
        }
    }

    write_suggestions(&mut text, "Proposals waiting for review", pending);
    write_suggestions(
        &mut text,
        "Proposals people dismissed, each with why",
        dismissed,
    );

    text.push_str("\nRecords ingested since the last review, oldest first:\n");
    let Some(room) = PROMPT_BUDGET_BYTES.checked_sub(text.len()) else {
        return Err(PromptError::DocTooLong);
    };
    if room < MIN_RECORDS_BYTES {
        return Err(PromptError::DocTooLong);
    }

    let mut records_included = 0;
    let mut records_bytes = 0;
    for record in records {
        let rendered = record_text(record);
        if records_bytes + rendered.len() > room {
            break;
        }
        records_bytes += rendered.len();
        text.push_str(&rendered);
        records_included += 1;
    }
    if records_included == 0 {
        return Err(PromptError::NoRecords);
    }
    Ok(Prompt {
        text,
        records_included,
    })
}

/// Lists `suggestions` under `heading`, each as its section, reason, why it was dismissed when it
/// was, and a diff of its change.
fn write_suggestions(text: &mut String, heading: &str, suggestions: &[SectionSuggestion]) {
    let _ = writeln!(text, "\n{heading}:");
    if suggestions.is_empty() {
        text.push_str("(none)\n");
        return;
    }
    for suggested in suggestions {
        let suggestion = &suggested.suggestion;
        let patch = diffy::create_patch(&suggestion.base_body, &suggestion.body).to_string();
        let _ = write!(
            text,
            "- {}: {}",
            suggested.section.as_str(),
            suggestion.reason
        );
        if let Some(why) = &suggestion.dismissal_reason {
            let _ = write!(text, " (dismissed: {why})");
        }
        let _ = writeln!(text, "\n{}", truncated(&patch, PROPOSAL_DIFF_CHARACTERS));
    }
}

/// One record as the prompt shows it: its id, where and when it came from, who wrote it, its
/// title, and its body cut short.
fn record_text(record: &TimelineRecord) -> String {
    let mut text = format!(
        "\n[record {}] {} at {}",
        record.id,
        record.source_kind,
        record.occurred_at.to_rfc3339()
    );
    if !record.actor.is_empty() {
        let _ = write!(text, " by {}", record.actor);
    }
    text.push('\n');
    if !record.title.is_empty() {
        let _ = writeln!(text, "{}", record.title);
    }
    let _ = writeln!(
        text,
        "{}",
        truncated(record.body.trim(), RECORD_BODY_CHARACTERS)
    );
    text
}

/// `text` cut to `limit` characters, marked when cut.
fn truncated(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(limit).collect();
    cut.push('…');
    cut
}

/// A proposal the model made that holds up, ready to propose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub section: Section,
    pub edits: Vec<Edit>,
    pub reason: String,
    pub quote: Option<String>,
    /// Where the quote is from, written from the record it cites.
    pub source: Option<String>,
    pub timeline_record_id: Option<Uuid>,
}

/// Why an answer could not be used at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnswerError {
    #[error("the answer holds no JSON object")]
    NoJson,
    #[error("the answer is not the expected shape: {0}")]
    Shape(String),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Answer {
    proposals: Vec<AnswerProposal>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnswerProposal {
    section: String,
    #[serde(default)]
    edits: Vec<AnswerEdit>,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    quote: Option<String>,
    #[serde(default)]
    record_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AnswerEdit {
    #[serde(default)]
    find: String,
    #[serde(default)]
    replace: String,
}

/// The proposals in the model's answer that hold up: for a section the doc has, with at least
/// one edit and a reason, the first for each section, citing only a record in `offered`.
///
/// # Errors
/// [`AnswerError`] when the answer holds no JSON object or is not the asked-for shape.
pub fn parse_answer(
    answer: &str,
    offered: &[TimelineRecord],
) -> Result<Vec<Suggestion>, AnswerError> {
    let json = json_object(answer).ok_or(AnswerError::NoJson)?;
    let parsed: Answer =
        serde_json::from_str(json).map_err(|error| AnswerError::Shape(error.to_string()))?;

    let mut proposed_sections = HashSet::new();
    let mut suggestions = Vec::new();
    for candidate in parsed.proposals {
        let Some(section) = Section::parse(candidate.section.trim()) else {
            continue;
        };
        let reason = truncated(candidate.reason.trim(), REASON_CHARACTERS);
        let edits: Vec<Edit> = candidate
            .edits
            .into_iter()
            .filter(|edit| !(edit.find.is_empty() && edit.replace.trim().is_empty()))
            .map(|edit| Edit {
                find: edit.find,
                replace: edit.replace,
            })
            .collect();
        if reason.is_empty() || edits.is_empty() || edits.len() > EDITS_MAX {
            continue;
        }
        if !proposed_sections.insert(section) {
            continue;
        }
        let cited = candidate
            .record_id
            .as_deref()
            .and_then(|id| Uuid::parse_str(id.trim()).ok())
            .and_then(|id| offered.iter().find(|record| record.id == id));
        let quote = candidate
            .quote
            .map(|quote| truncated(quote.trim(), QUOTE_CHARACTERS))
            .filter(|quote| !quote.is_empty());
        suggestions.push(Suggestion {
            section,
            edits,
            reason,
            quote,
            source: cited.map(source_of),
            timeline_record_id: cited.map(|record| record.id),
        });
    }
    Ok(suggestions)
}

/// How a proposal names the record it cites: its title and where it came from, and who wrote
/// it. The record stays its owner's, so this is what the doc shows of it.
fn source_of(record: &TimelineRecord) -> String {
    let mut source = record.source_kind.clone();
    if !record.actor.is_empty() {
        let _ = write!(source, ", {}", record.actor);
    }
    if !record.title.is_empty() {
        source = format!("{} ({source})", record.title);
    }
    truncated(&source, SOURCE_CHARACTERS)
}

/// The first `{` to the last `}`: models sometimes wrap JSON in a sentence or a code fence.
fn json_object(answer: &str) -> Option<&str> {
    let start = answer.find('{')?;
    let end = answer.rfind('}')?;
    (end > start).then(|| &answer[start..=end])
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    fn record(title: &str, body: &str) -> TimelineRecord {
        TimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "slack".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: Utc::now(),
            actor: "Priya Raman".to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            url: None,
            media: None,
            metadata: json!({}),
            action_item_id: None,
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            excerpt_mark_id: None,
            deleted_at: None,
        }
    }

    fn sections(content: &str) -> Vec<DocSection> {
        Section::ALL
            .into_iter()
            .map(|section| DocSection {
                section,
                content: content.to_owned(),
                updated_at: Utc::now(),
            })
            .collect()
    }

    #[test]
    fn the_prompt_holds_the_doc_whole_and_as_many_records_as_fit() {
        let records: Vec<TimelineRecord> = (0..200)
            .map(|index| record(&format!("Message {index}"), &"word ".repeat(400)))
            .collect();

        let prompt =
            prompt(&sections("- Priya Raman, operations"), &[], &[], &records).expect("builds");

        assert!(prompt.text.contains("## Glossary (section key: glossary)"));
        assert!(prompt.text.contains("- Priya Raman, operations"));
        assert!(prompt.records_included > 0 && prompt.records_included < records.len());
        assert!(prompt.text.len() <= PROMPT_BUDGET_BYTES);
        assert!(
            prompt.text.contains(&records[0].id.to_string()),
            "oldest first"
        );
    }

    #[test]
    fn a_doc_that_leaves_no_room_for_records_is_refused() {
        let full = "x".repeat(18_000);
        let refused = prompt(&sections(&full), &[], &[], &[record("A", "B")]);
        assert_eq!(refused, Err(PromptError::DocTooLong));
        assert_eq!(
            prompt(&sections(""), &[], &[], &[]),
            Err(PromptError::NoRecords)
        );
    }

    #[test]
    fn an_answer_keeps_one_proposal_per_known_section_and_cites_only_offered_records() {
        let offered = record("#general", "I'm Priya Raman, spelled with an a.");
        let answer = json!({
            "proposals": [
                {
                    "section": "people",
                    "edits": [{ "find": "", "replace": "- Priya Raman, operations" }],
                    "reason": "She introduced herself.",
                    "quote": "I'm Priya Raman",
                    "recordId": offered.id.to_string(),
                },
                {
                    "section": "people",
                    "edits": [{ "find": "", "replace": "- Someone else" }],
                    "reason": "A second one for the same section.",
                },
                {
                    "section": "history",
                    "edits": [{ "find": "", "replace": "x" }],
                    "reason": "Not a section.",
                },
                {
                    "section": "glossary",
                    "edits": [],
                    "reason": "Nothing to change.",
                },
                {
                    "section": "products",
                    "edits": [{ "find": "", "replace": "- Atlas" }],
                    "reason": "Named in a record never offered.",
                    "recordId": Uuid::now_v7().to_string(),
                },
            ],
        });
        let answer = format!("Here you go:\n```json\n{answer}\n```");

        let suggestions = parse_answer(&answer, std::slice::from_ref(&offered)).expect("parses");

        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].section, Section::People);
        assert_eq!(suggestions[0].timeline_record_id, Some(offered.id));
        assert_eq!(
            suggestions[0].source.as_deref(),
            Some("#general (slack, Priya Raman)")
        );
        assert_eq!(suggestions[1].section, Section::Products);
        assert_eq!(
            suggestions[1].timeline_record_id, None,
            "a record that was not offered is not cited"
        );
    }

    #[test]
    fn an_answer_without_json_or_of_another_shape_is_refused() {
        assert_eq!(parse_answer("Nothing new.", &[]), Err(AnswerError::NoJson));
        assert!(matches!(
            parse_answer(r#"{"segments": []}"#, &[]),
            Err(AnswerError::Shape(_))
        ));
        assert_eq!(parse_answer(r#"{"proposals": []}"#, &[]), Ok(Vec::new()));
    }
}
