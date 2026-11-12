// Copyright © 2026 Jalapeno Labs

//! A person's edit of a meeting's words, which teaches the Transcription doc when it fixes a name
//! or a term. See `docs/meetings.md`, The Transcription doc.
//!
//! Every edit of a meeting's words comes through [`edit`], so a fix teaches the doc the moment it
//! is made, in the same transaction: an edit that only fixes words (`correction::check_fix`, as a
//! correction's own fixes must) adds each of its fixes to the doc as an entry, or strengthens the
//! entry it already has, as a version of the doc by the person who made it, with where it was heard
//! and who was speaking. A fix naming someone or something the company doc does not know is
//! suggested to it, for a person to accept. An edit that rewrites more than that, or that only
//! changes case and punctuation, teaches nothing. Undoing an edit leaves what it taught, which a
//! person prunes from the doc as any entry.

use diesel_async::{AsyncConnection, AsyncPgConnection};

use super::{comments, correction};
use crate::models::agent::Author;
use crate::models::meeting::Meeting;
use crate::models::meeting_event::MeetingEvent;
use crate::models::meeting_word::{self, EditError, Origin, Replacement, WordChange};
use crate::models::{meeting_capture, user};
use crate::realtime::{EventBus, ServerEvent};
use crate::routes::v1::pages;
use crate::transcription_doc::{self, Lesson, MeetingLink};

/// Replaces the words of `replacement.range` in `meeting` with its text as `author`
/// (`comments::edit_words`, which indexes again the segments holding a thread that quotes them),
/// and teaches the Transcription doc what it fixed, in one transaction; then announces the doc's
/// change and any suggestion to the company doc. Answers the edit's history entry.
///
/// # Errors
/// Every refusal of `meeting_word::edit`, and any database error.
pub async fn edit(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    meeting: &Meeting,
    replacement: Replacement<'_>,
    author: Author<'_>,
) -> Result<MeetingEvent, EditError> {
    let (entry, learned, suggested) = connection
        .transaction(async |connection| {
            let entry = comments::edit_words(connection, meeting, replacement, author).await?;
            let lessons = lessons_of(connection, &entry, meeting, author).await?;
            let learned = transcription_doc::learn(connection, &lessons, author).await?;
            let people = people_of(connection, meeting).await?;
            let suggested =
                transcription_doc::suggest_unknown(connection, &lessons, &people, (author, None))
                    .await?;
            Ok::<_, EditError>((entry, learned, suggested))
        })
        .await?;
    if learned {
        events.publish(&ServerEvent::PageChanged {
            id: transcription_doc::TRANSCRIPTION_PAGE_ID,
        });
    }
    for each in suggested {
        pages::announce_suggested(events, connection, each).await;
    }
    Ok(entry)
}

/// What `entry`, an edit `author` made in `meeting`, teaches the Transcription doc: each of its
/// fixes, heard by the transcript and speaker of the first word it replaced. None when it is no
/// fix.
async fn lessons_of(
    connection: &mut AsyncPgConnection,
    entry: &MeetingEvent,
    meeting: &Meeting,
    author: Author<'_>,
) -> diesel::QueryResult<Vec<Lesson>> {
    let change: WordChange = serde_json::from_value(entry.data.clone())
        .expect("an edit records its change in its own shape");
    let Ok(fixes) = correction::check_fix(&change.before_text, &change.after_text) else {
        // A rewrite, not a mishearing: nothing for the doc to learn.
        return Ok(Vec::new());
    };
    if fixes.is_empty() {
        return Ok(Vec::new());
    }
    let replaced = meeting_word::find_many(connection, entry.meeting_id, &change.replaced).await?;
    let first = replaced
        .first()
        .expect("an edit replaces at least one standing word");
    let source = if first.origin == Origin::Whisper.as_str() {
        Some("Whisper".to_owned())
    } else {
        match first.capture_id {
            Some(capture_id) => {
                let capture = meeting_capture::find(connection, capture_id).await?;
                Some(super::source_name(&capture.source_kind))
            }
            None => None,
        }
    };
    // The Transcription doc is every member's page, so a private meeting teaches its fixes alone:
    // a link with no title, and not who spoke, which can say what made it private.
    let public = meeting.public_title().is_some();
    let speaker = Some(first.speaker.clone()).filter(|speaker| public && !speaker.is_empty());
    let person = user::find(connection, author.user_id).await?;
    let name = Some(person.name.trim())
        .filter(|name| !name.is_empty())
        .unwrap_or("A person");
    let why = format!("{name} fixed it on the meeting's page.");
    Ok(fixes
        .into_iter()
        .map(|fix| Lesson {
            heard: fix.heard,
            written: fix.written,
            source: source.clone(),
            speaker: speaker.clone(),
            meeting: MeetingLink {
                id: meeting.id,
                title: meeting.public_title().unwrap_or_default().to_owned(),
            },
            why: why.clone(),
        })
        .collect())
}

/// Who took part in `meeting`: its participants and the speakers of its words, as the company
/// doc's People are told apart from its Glossary.
async fn people_of(
    connection: &mut AsyncPgConnection,
    meeting: &Meeting,
) -> diesel::QueryResult<Vec<String>> {
    let mut people: Vec<String> =
        serde_json::from_value(meeting.participants.clone()).unwrap_or_default();
    let words = meeting_word::standing(connection, meeting.id).await?;
    people.extend(words.into_iter().map(|word| word.speaker));
    people.retain(|person| !person.trim().is_empty());
    people.sort();
    people.dedup();
    Ok(people)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::meeting_mark::Range;
    use crate::models::meeting_word::tests::{built_meeting, person, word};
    use crate::models::page;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_persons_fix_teaches_the_transcription_doc_at_once_and_a_rewrite_teaches_nothing() {
        let (_url, mut connection) = migrated_database().await;
        let events = EventBus::in_process();
        let (capture, words) = built_meeting(
            &mut connection,
            &[
                ("Ada", "We asked Sheriff about it"),
                ("Grace", "Then we shipped"),
            ],
        )
        .await;
        // Open to every member, so the doc may name it and who spoke; a private meeting teaches
        // its fixes alone (`meetings::correct`'s tests).
        let found = crate::models::meeting::set_visibility(
            &mut connection,
            capture.meeting_id,
            crate::models::meeting::Visibility::Public,
        )
        .await
        .expect("the meeting opens");
        let sheriff = word(&words, "Sheriff");
        let one_word = Range {
            start_word_id: sheriff.id,
            end_word_id: sheriff.id,
        };

        edit(
            &mut connection,
            &events,
            &found,
            Replacement {
                range: one_word,
                text: "Sharath",
            },
            person(),
        )
        .await
        .expect("edits");

        let doc = transcription_doc::read(&mut connection)
            .await
            .expect("the doc");
        assert_eq!(doc.entries.len(), 1, "{}", doc.body);
        let entry = &doc.entries[0];
        assert_eq!(
            (entry.heard.as_str(), entry.written.as_str()),
            ("Sheriff", "Sharath")
        );
        assert_eq!(entry.sources, vec!["Fireflies"]);
        assert_eq!(entry.speakers, vec!["Ada"]);
        assert_eq!(entry.meetings[0].id, capture.meeting_id);
        assert!(doc.body.contains("Tester fixed it on the meeting's page."));
        let versions = page::versions(&mut connection, transcription_doc::TRANSCRIPTION_PAGE_ID)
            .await
            .expect("versions");
        assert_eq!(
            (versions[0].version, versions[0].created_by),
            (2, TEST_PERSON_ID),
            "a version of the doc by the person who fixed it"
        );

        // Rewriting a sentence is no mishearing, and the doc learns nothing from it.
        let words = meeting_word::standing(&mut connection, capture.meeting_id)
            .await
            .expect("reads");
        let sentence = Range {
            start_word_id: word(&words, "Then").id,
            end_word_id: word(&words, "shipped").id,
        };
        edit(
            &mut connection,
            &events,
            &found,
            Replacement {
                range: sentence,
                text: "After all that, everyone agreed to ship it on Friday",
            },
            person(),
        )
        .await
        .expect("edits");
        let again = transcription_doc::read(&mut connection)
            .await
            .expect("the doc");
        assert_eq!(again, doc);
    }
}
