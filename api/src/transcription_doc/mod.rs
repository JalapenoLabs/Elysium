// Copyright © 2026 Jalapeno Labs

//! The Transcription doc: how speech is misheard in the company's meetings, and what it is. See
//! `docs/meetings.md`, The Transcription doc.
//!
//! One fixed page of the knowledge base ([`TRANSCRIPTION_PAGE_ID`]), beside Learnings and the
//! Company page, is the one source of truth for it, such as "Sheriff" heard for "Sharath" in Teams.
//! It is read by every correction ([`read`], the whole page in its prompt) and by every
//! transcription ([`hint_words`], the names Whisper is told to listen for), and it teaches itself:
//! a person's fix on a meeting's page and a correction the correcting agent is sure of each add an
//! entry or strengthen one ([`learn`]), live at once, as a version of the page by whoever taught
//! it. People edit it as any page; what Elysium reads and writes of its Markdown is `markdown`'s.
//!
//! A lesson naming someone or something the company doc does not know is also proposed to the
//! company doc ([`propose_unknown`]), for a person to approve, as every other change to it is.

mod markdown;

use diesel::QueryResult;
use diesel_async::{AsyncConnection, AsyncPgConnection};
use tracing::{Level, event};

use crate::models::agent::Author;
use crate::models::company_doc::{
    self, CompanyDocError, Edit, NewProposal, ProposalState, Proposer, Section,
};
use crate::models::page::{self, PageContent, PageError, Provenance};

pub use crate::models::page::TRANSCRIPTION_PAGE_ID;
pub use markdown::{Entry, Lesson, MeetingLink, phrase_key};

/// The most characters of hint words Whisper is given, joined by commas as it reads them. Whisper
/// reads them as the start of its own context, which holds at most 224 tokens, and faster-whisper
/// keeps only the last of them; this fits with room for what Whisper heard before.
const HINT_WORDS_MAX_CHARACTERS: usize = 500;

/// The longest company doc line read as a name or a term: past this, a line is a sentence.
const TERM_MAX_CHARACTERS: usize = 60;

/// The doc as it reads now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionDoc {
    /// Its whole text, as a correction's prompt carries it: the entries and every note a person
    /// added beside them.
    pub body: String,
    pub entries: Vec<Entry>,
}

/// The doc as it reads now.
///
/// # Errors
/// Propagates any database error, such as a database whose migrations did not seed the page.
pub async fn read(connection: &mut AsyncPgConnection) -> QueryResult<TranscriptionDoc> {
    let doc = page::find(connection, TRANSCRIPTION_PAGE_ID).await?;
    Ok(TranscriptionDoc {
        entries: markdown::entries(&doc.body),
        body: doc.body,
    })
}

/// Teaches the doc `lessons` as `author`, under its page's lock, as one version of it by them.
/// Answers whether it changed: a lesson the doc already holds teaches it nothing. The caller
/// announces the page's change once its transaction commits.
///
/// # Errors
/// Propagates any database error.
pub async fn learn(
    connection: &mut AsyncPgConnection,
    lessons: &[Lesson],
    author: Author<'_>,
) -> QueryResult<bool> {
    if lessons.is_empty() {
        return Ok(false);
    }
    let published = connection
        .transaction(async |connection| {
            let doc = page::lock(connection, TRANSCRIPTION_PAGE_ID).await?;
            let body = markdown::teach(&doc.body, lessons, author.at.date_naive());
            if body == doc.body {
                return Ok(false);
            }
            let content = PageContent {
                body,
                ..doc.content()
            };
            page::publish_content(connection, &doc, content, Provenance::default(), author).await?;
            Ok(true)
        })
        .await;
    match published {
        Ok(changed) => Ok(changed),
        Err(PageError::Database(error)) => Err(error),
        // The doc is no learning and no section, so a publish has nothing else to refuse; were it
        // to, the fix that taught it still stands.
        Err(refused) => {
            event!(
                name: "transcription_doc.learning.refused",
                Level::WARN,
                error.message = %refused,
                "the Transcription doc refused what a fix taught it",
            );
            Ok(false)
        }
    }
}

/// The names and terms Whisper is told to listen for before it transcribes: what the doc's entries
/// are, those heard in the most meetings first, then the names and terms the company doc's People
/// and Glossary sections spell, each once.
///
/// # Errors
/// Propagates any database error.
pub async fn hint_words(connection: &mut AsyncPgConnection) -> QueryResult<Vec<String>> {
    let doc = read(connection).await?;
    let sections = company_doc::sections(connection).await?;
    let section_text = |wanted: Section| {
        sections
            .iter()
            .find(|section| section.section == wanted)
            .map(|section| section.content.as_str())
            .unwrap_or_default()
    };
    Ok(hints(
        &doc.entries,
        section_text(Section::People),
        section_text(Section::Glossary),
    ))
}

/// The hint words of `entries`, then of the company doc's `people` and `glossary` sections, within
/// [`HINT_WORDS_MAX_CHARACTERS`] once joined by commas. See [`hint_words`].
fn hints(entries: &[Entry], people: &str, glossary: &str) -> Vec<String> {
    let mut by_strength: Vec<&Entry> = entries.iter().collect();
    // A stable sort keeps the doc's own order among entries heard as often.
    by_strength.sort_by_key(|entry| std::cmp::Reverse(entry.meetings.len()));
    let terms = by_strength
        .into_iter()
        .map(|entry| entry.written.trim().to_owned())
        .chain(people.lines().chain(glossary.lines()).filter_map(term_of));

    let mut seen = std::collections::HashSet::new();
    let mut hints = Vec::new();
    let mut characters = 0;
    for term in terms {
        let key = phrase_key(&term);
        if key.is_empty() || !seen.insert(key) {
            continue;
        }
        let separator = if hints.is_empty() { 0 } else { ", ".len() };
        characters += separator + term.chars().count();
        if characters > HINT_WORDS_MAX_CHARACTERS {
            break;
        }
        hints.push(term);
    }
    hints
}

/// The name or term a company doc line spells, as its People and Glossary sections write them one
/// per line ("- Ada Lovelace (Ada): CTO", "- ACME: our first client"): what comes before the first
/// colon, dash, bracket, or comma, without list marks or emphasis. `None` for a heading, a blank
/// line, or a sentence.
fn term_of(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let unmarked = line
        .trim_start_matches(|character: char| {
            matches!(character, '-' | '*' | '+') || character.is_ascii_digit()
        })
        .trim_start_matches(['.', ')'])
        .trim();
    let end = [": ", " - ", " — ", " (", ", ", "; "]
        .iter()
        .filter_map(|stop| unmarked.find(stop))
        .min()
        .unwrap_or(unmarked.len());
    let term = unmarked[..end]
        .trim_end_matches(':')
        .trim_matches(['*', '_', '`'])
        .trim();
    let fits = !term.is_empty()
        && term.chars().count() <= TERM_MAX_CHARACTERS
        && term.split_whitespace().count() <= 6;
    fits.then(|| term.to_owned())
}

/// Proposes to the company doc each of `lessons` whose written form it does not name yet, nor any
/// proposal waiting for review does: to People when it names one of `people` (the meeting's
/// speakers and participants), and to the Glossary otherwise, each as one line a person approves
/// or rejects. Answers how many it proposed; the caller announces the doc's change once its
/// transaction commits.
///
/// A proposal the doc refuses, one that would make a section too long, is logged and passed over:
/// the lesson is in the Transcription doc whatever the company doc says.
///
/// # Errors
/// Propagates any database error.
pub async fn propose_unknown(
    connection: &mut AsyncPgConnection,
    lessons: &[Lesson],
    people: &[String],
    proposer: Proposer<'_>,
) -> QueryResult<usize> {
    if lessons.is_empty() {
        return Ok(0);
    }
    let sections = company_doc::sections(connection).await?;
    let pending = company_doc::proposals(connection, ProposalState::Pending, None).await?;
    let mut known: Vec<String> = sections
        .iter()
        .filter(|section| matches!(section.section, Section::People | Section::Glossary))
        .map(|section| phrase_key(&section.content))
        .collect();
    known.extend(
        pending
            .iter()
            .filter(|proposal| {
                Section::parse(&proposal.section)
                    .is_some_and(|section| matches!(section, Section::People | Section::Glossary))
            })
            .map(|proposal| phrase_key(&proposal.content)),
    );
    let person_keys: Vec<String> = people.iter().map(|person| name_key(person)).collect();

    let mut created = 0;
    for lesson in lessons {
        let written = phrase_key(&lesson.written);
        if written.is_empty() || known.iter().any(|text| holds_words(text, &written)) {
            continue;
        }
        let section = if person_keys.iter().any(|name| holds_words(name, &written)) {
            Section::People
        } else {
            Section::Glossary
        };
        let written_form = lesson.written.trim();
        let new = NewProposal {
            section,
            edits: vec![Edit {
                find: String::new(),
                replace: format!(
                    "- {written_form}: often heard as \"{}\"",
                    lesson.heard.trim()
                ),
            }],
            reason: format!(
                "A meeting's transcript was corrected: \"{}\" is {written_form}, which the company \
                 doc does not name yet.",
                lesson.heard.trim()
            ),
            quote: None,
            source: Some(lesson.meeting.title.chars().take(200).collect()),
            timeline_record_id: None,
        };
        match company_doc::propose(connection, new, proposer, chrono::Utc::now()).await {
            Ok(stored) => {
                created += usize::from(stored.created);
                known.push(written);
            }
            Err(CompanyDocError::Database(error)) => return Err(error),
            Err(refused) => event!(
                name: "transcription_doc.proposal.refused",
                Level::DEBUG,
                company_doc.section = section.as_str(),
                error.message = %refused,
                "a name a correction taught could not be proposed to the company doc",
            ),
        }
    }
    Ok(created)
}

/// A person as names are compared: an email's part before the `@`, its dots and underscores read
/// as spaces, since participants are often listed by email.
fn name_key(person: &str) -> String {
    let name = person.split('@').next().unwrap_or(person);
    phrase_key(&name.replace(['.', '_'], " "))
}

/// Whether `text`, a phrase key, holds `words`, another, as whole words.
fn holds_words(text: &str, words: &str) -> bool {
    format!(" {text} ").contains(&format!(" {words} "))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn entry(written: &str, meetings: usize) -> Entry {
        Entry {
            heard: format!("not {written}"),
            written: written.to_owned(),
            sources: Vec::new(),
            speakers: Vec::new(),
            meetings: (0..meetings)
                .map(|_| MeetingLink {
                    id: Uuid::now_v7(),
                    title: "Standup".to_owned(),
                })
                .collect(),
        }
    }

    #[test]
    fn whisper_hears_the_most_taught_names_first_then_the_company_docs() {
        let entries = vec![entry("Elysium", 1), entry("Sharath", 3)];
        let people = "- Ada Lovelace (Ada): CTO\n- **Sharath Kumar**: infrastructure\n\n## Former";
        let glossary = "- ACME: our first client\n- elysium: the product\n\
                        This line explains how the glossary is kept, and is no term at all.";

        assert_eq!(
            hints(&entries, people, glossary),
            vec![
                "Sharath",
                "Elysium",
                "Ada Lovelace",
                "Sharath Kumar",
                "ACME"
            ]
        );
    }

    #[test]
    fn hint_words_stop_before_whispers_context_is_full() {
        let entries: Vec<Entry> = (0..200)
            .map(|number| entry(&format!("Name{number}"), 1))
            .collect();
        let hint = hints(&entries, "", "").join(", ");
        assert!(hint.chars().count() <= HINT_WORDS_MAX_CHARACTERS);
        assert!(hint.chars().count() > HINT_WORDS_MAX_CHARACTERS - "Name199, ".len());
        assert!(hint.starts_with("Name0, Name1"));
    }

    #[test]
    fn a_company_doc_line_reads_as_the_name_it_spells() {
        assert_eq!(
            term_of("- Ada Lovelace (Ada): CTO").as_deref(),
            Some("Ada Lovelace")
        );
        assert_eq!(
            term_of("* `Elysium` - the product").as_deref(),
            Some("Elysium")
        );
        assert_eq!(term_of("1. ACME, our client").as_deref(), Some("ACME"));
        assert_eq!(term_of("## People"), None);
        assert_eq!(term_of("   "), None);
        assert_eq!(
            term_of("- Everyone here goes by their first name in meetings and in chat"),
            None,
            "a sentence is no name"
        );
    }

    #[test]
    fn a_person_is_known_by_their_name_or_their_email() {
        let names: Vec<String> = ["sharath.kumar@mooreslab.ai", "Ada Lovelace"]
            .into_iter()
            .map(name_key)
            .collect();
        assert!(names.iter().any(|name| holds_words(name, "sharath")));
        assert!(names.iter().any(|name| holds_words(name, "ada lovelace")));
        assert!(!names.iter().any(|name| holds_words(name, "lab")));
    }
}
