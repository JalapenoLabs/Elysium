// Copyright © 2026 Jalapeno Labs

//! How a timeline record is indexed: the one document each record becomes.
//!
//! Every kind of record is indexed the same way, one record to one document, so the index holds
//! a meeting's excerpts (each a record of its own, `docs/meetings.md`), a
//! conversation by its messages, and a page by its newest version (Revisions, below). A kind
//! recorded later, such as Jira's or GitHub's issues, is indexed the moment it is on the
//! timeline, with nothing added here.
//!
//! # The text
//!
//! A record's document is its title, who it came from, and its body, in that order, cut into
//! passages by [`passages::split`]. A meeting excerpt's speakers are already named on every
//! line of its excerpt, so it gives its summary in their place, and is followed by the comment
//! threads whose range overlaps it (`crate::meetings::comments`), each the first words of its
//! range and its comments by their authors' names, so a meeting's comments are indexed with it
//! and go wherever its excerpts go. Then come the files and frames pinned over its span
//! (`crate::meetings::attachments`): each frame's caption, and each file's name and the text read
//! from it, so what a meeting showed travels with the excerpts that discussed it. A record's
//! metadata and media are never indexed: they hold the provider's ids and where a clip is kept,
//! not what was said.
//! A body past [`BODY_MAX_CHARACTERS`] is cut there, which every source today stays inside
//! (Confluence and sent mail keep 20,000 characters, Slack 4,000, webmail and Teams 1,000)
//! except a long meeting excerpt.
//!
//! # Revisions
//!
//! Confluence records each version of a page or comment as a record of its own, so the timeline
//! shows every edit. The index holds what a page says now: of a page's versions, only the newest
//! has a document ([`revision`], and `timeline_record::index`).
//!
//! # Re-indexing
//!
//! [`INDEXED_AS`] is the version of all of this. A record is indexed as it is stored, and never
//! again unless it changes, so a release that changes what a record's text is bumps it, and the
//! engine's backfill (`super::backfill`) indexes every record again; one whose text came out the
//! same keeps its passages and their embeddings.

use tracing::{Level, event};
use uuid::Uuid;

use crate::connectors::ConnectorKind;
use crate::meetings::SEGMENT_SOURCE_KIND;
use crate::models::knowledge_document::{IndexedDocument, KnowledgeSource};
use crate::models::meeting_attachment::{AttachmentKind, IndexedAttachment};
use crate::models::meeting_comment::IndexedThread;
use crate::models::timeline_record::TimelineRecord;

use super::passages;

/// The version of how a record becomes a document. Bump it whenever [`document`] changes what a
/// record's text is, or [`passages::split`] how it is cut, so the backfill brings every record
/// stored before onto the new text.
pub const INDEXED_AS: i32 = 1;

/// The most of a record's body that is indexed, in characters: about 63 passages, which the
/// embedding model takes some fifteen seconds over on four CPUs. A two hour meeting spent on one
/// thing is about this long, so every excerpt is indexed whole while one pathological body
/// cannot hold the embedder for minutes. Raising it re-indexes nothing already indexed.
pub const BODY_MAX_CHARACTERS: usize = 100_000;

/// The most of an excerpt's comment threads indexed after its body, in characters, so an excerpt
/// with many long threads holds the embedder no longer than a long body does. Threads are taken in
/// the order their ranges start, so what is left out is the last of them.
pub const THREADS_MAX_CHARACTERS: usize = 50_000;

/// The most of the files and frames pinned in an excerpt indexed after its threads, in
/// characters, as for its threads: a long document's text pinned in one excerpt is indexed with
/// the meeting whole, so the excerpt needs only its start to be found by it.
pub const PINNED_MAX_CHARACTERS: usize = 50_000;

/// The document `record` is indexed as, with `threads`, the comment threads over a meeting
/// segment, after its body, and then `pinned`, the files and frames pinned over it; `None` when it
/// says nothing to search (no title, no sender, and no body), and for a meeting segment that names
/// no meeting, which nobody could be told may read it.
pub fn document(
    record: &TimelineRecord,
    threads: &[IndexedThread],
    pinned: &[IndexedAttachment],
) -> Option<IndexedDocument> {
    // An excerpt is found only by its meeting's readers, so one naming no meeting is indexed for
    // nobody rather than for every member.
    let access_meeting_id = if record.source_kind == SEGMENT_SOURCE_KIND {
        Some(meeting_of(record)?)
    } else {
        None
    };
    let mut paragraphs: Vec<String> = vec![record.title.clone()];
    if record.source_kind == SEGMENT_SOURCE_KIND {
        if let Some(summary) = record.metadata["summary"].as_str() {
            paragraphs.push(summary.to_owned());
        }
    } else if !record.actor.trim().is_empty() {
        paragraphs.push(format!("From {}", record.actor.trim()));
    }
    paragraphs.push(record.body.chars().take(BODY_MAX_CHARACTERS).collect());
    // After the body, so a comment written later changes only the passages at the document's end
    // and the rest keep their embeddings.
    paragraphs.extend(thread_paragraphs(threads));
    paragraphs.extend(attachment_paragraphs(pinned, PINNED_MAX_CHARACTERS));

    let passages = passages::split(&paragraphs.join("\n\n"));
    if passages.is_empty() {
        return None;
    }
    Some(IndexedDocument {
        source: KnowledgeSource::TimelineRecord {
            id: record.id,
            kind: record.source_kind.clone(),
        },
        title: record.title.clone(),
        url: record.url.clone(),
        occurred_at: record.occurred_at,
        // Whose timeline it is on: whose connector read it, or who sent it.
        created_by: record.created_by,
        access_meeting_id,
        passages,
    })
}

/// The meeting `record`, an excerpt, is of, as its metadata names it, whose readers alone find
/// its document (`docs/knowledge.md`, Who sees what). An excerpt's record names it always, so `None`
/// is a fault, and the excerpt is left out of the index.
fn meeting_of(record: &TimelineRecord) -> Option<Uuid> {
    let meeting_id = record.metadata["meetingId"]
        .as_str()
        .and_then(|meeting_id| meeting_id.parse().ok());
    if meeting_id.is_none() {
        event!(
            name: "knowledge.records.meeting_unnamed",
            Level::WARN,
            timeline_record.id = %record.id,
            "a meeting excerpt names no meeting, so nobody finds it",
        );
    }
    meeting_id
}

/// `threads` as the paragraphs a document holds after its text, in order, up to
/// [`THREADS_MAX_CHARACTERS`]: each the first words of its range, then its comments by their
/// authors' names.
pub fn thread_paragraphs(threads: &[IndexedThread]) -> Vec<String> {
    let mut paragraphs = Vec::new();
    let mut room = THREADS_MAX_CHARACTERS;
    for thread in threads {
        let mut paragraph = format!("Comment on \"{}\"", thread.quote);
        for comment in &thread.comments {
            paragraph.push('\n');
            paragraph.push_str(&comment.author);
            paragraph.push_str(": ");
            paragraph.push_str(&comment.body);
        }
        let length = paragraph.chars().count();
        if length > room {
            paragraphs.push(paragraph.chars().take(room).collect());
            break;
        }
        room -= length;
        paragraphs.push(paragraph);
    }
    paragraphs
}

/// `attachments` as the paragraphs a document holds after its threads, in order, up to `room`
/// characters: a frame as its caption, and a file as its name and then the text read from it.
pub fn attachment_paragraphs(attachments: &[IndexedAttachment], mut room: usize) -> Vec<String> {
    let mut paragraphs = Vec::new();
    for attachment in attachments {
        let mut paragraph = match attachment.kind {
            AttachmentKind::Frame => format!(
                "Frame shown: {}",
                attachment.caption.as_deref().unwrap_or_default()
            ),
            AttachmentKind::File => format!("Attached file: {}", attachment.file_name),
        };
        if let Some(text) = attachment.text.as_deref().map(str::trim)
            && !text.is_empty()
        {
            paragraph.push_str("\n\n");
            paragraph.push_str(text);
        }
        let length = paragraph.chars().count();
        if length > room {
            paragraphs.push(paragraph.chars().take(room).collect());
            break;
        }
        room -= length;
        paragraphs.push(paragraph);
    }
    paragraphs
}

/// What a record is one version of, for a kind recorded once per version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision<'record> {
    /// The connector whose records the versions are.
    pub connector_id: Uuid,
    /// The external id prefix every version shares, such as `page:42:`.
    pub prefix: &'record str,
    /// Which version this record is; a newer version is a larger number.
    pub number: u64,
}

/// What `record` is a version of, when its kind records every version of a thing and the index
/// holds only the newest: Confluence's pages and comments. `None` for every other record, and
/// for one whose connector was removed, since its versions can no longer be told from another
/// site's.
pub fn revision(record: &TimelineRecord) -> Option<Revision<'_>> {
    if record.source_kind != ConnectorKind::Confluence.as_str() {
        return None;
    }
    let connector_id = record.connector_id?;
    let Some((prefix, number)) = record
        .external_id
        .as_deref()
        .and_then(crate::confluence::reader::version_of)
    else {
        event!(
            name: "knowledge.record.unversioned",
            Level::DEBUG,
            record.id = %record.id,
            "a Confluence record's external id names no version; indexing it on its own",
        );
        return None;
    };
    Some(Revision {
        connector_id,
        prefix,
        number,
    })
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;
    use crate::models::meeting_comment::IndexedComment;

    fn record(source_kind: &str) -> TimelineRecord {
        TimelineRecord {
            id: Uuid::now_v7(),
            source_kind: source_kind.to_owned(),
            connector_id: Some(Uuid::now_v7()),
            external_id: Some("C1/1730000000.000100".to_owned()),
            occurred_at: Utc::now(),
            actor: "Ada Lovelace".to_owned(),
            title: "#launch".to_owned(),
            body: "WAY-123 ships Friday.".to_owned(),
            url: Some("https://acme.slack.com/archives/C1/p1730000000000100".to_owned()),
            media: None,
            metadata: json!({ "team": "T1", "conversation": "C1" }),
            action_item_id: None,
            created_by: Uuid::now_v7(),
            created_at: Utc::now(),
            excerpt_mark_id: None,
            deleted_at: None,
        }
    }

    #[test]
    fn a_message_is_its_title_sender_and_body_attributed_to_its_owner() {
        let message = record("slack");
        let indexed = document(&message, &[], &[]).expect("a document");

        assert_eq!(
            indexed.passages,
            ["#launch\n\nFrom Ada Lovelace\n\nWAY-123 ships Friday."]
        );
        assert_eq!(
            indexed.source,
            KnowledgeSource::TimelineRecord {
                id: message.id,
                kind: "slack".to_owned(),
            }
        );
        assert_eq!(indexed.source.label(), "slack");
        assert_eq!(indexed.title, "#launch");
        assert_eq!(indexed.url, message.url);
        assert_eq!(indexed.occurred_at, message.occurred_at);
        assert_eq!(indexed.created_by, message.created_by);
    }

    #[test]
    fn metadata_is_never_indexed() {
        let mut message = record("slack");
        message.metadata = json!({ "conversation": "C0SECRETID", "draftId": Uuid::now_v7() });
        let indexed = document(&message, &[], &[]).expect("a document");

        assert!(
            indexed
                .passages
                .iter()
                .all(|passage| !passage.contains("C0SECRETID"))
        );
    }

    #[test]
    fn a_meeting_segment_gives_its_summary_in_place_of_its_speakers() {
        let mut segment = record(SEGMENT_SOURCE_KIND);
        segment.actor = "Ada, Grace".to_owned();
        segment.title = "Launch date".to_owned();
        segment.body = "[1:05] Ada: Friday works.\n[1:20] Grace: Friday it is.".to_owned();
        let meeting_id = Uuid::now_v7();
        segment.metadata = json!({
            "meetingId": meeting_id,
            "summary": "The launch moved to Friday.",
            "segmentIndex": 2,
        });

        let indexed = document(&segment, &[], &[]).expect("a document");

        assert_eq!(
            indexed.passages,
            ["Launch date\n\nThe launch moved to Friday.\n\n\
                 [1:05] Ada: Friday works.\n[1:20] Grace: Friday it is."]
        );
        assert_eq!(indexed.source.label(), "meeting");
        assert_eq!(
            indexed.access_meeting_id,
            Some(meeting_id),
            "found by its readers alone"
        );
    }

    #[test]
    fn a_segment_naming_no_meeting_is_indexed_for_nobody() {
        let mut segment = record(SEGMENT_SOURCE_KIND);
        segment.metadata = json!({ "summary": "The launch moved to Friday." });

        assert!(document(&segment, &[], &[]).is_none());
        assert_eq!(
            document(&record("slack"), &[], &[])
                .expect("a document")
                .access_meeting_id,
            None
        );
    }

    #[test]
    fn a_segments_comment_threads_follow_its_text() {
        let mut segment = record(SEGMENT_SOURCE_KIND);
        segment.title = "Launch date".to_owned();
        segment.body = "[1:05] Sheriff: Friday works.".to_owned();
        segment.metadata =
            json!({ "meetingId": Uuid::now_v7(), "summary": "The launch moved to Friday." });
        let threads = [IndexedThread {
            quote: "Sheriff: Friday works.".to_owned(),
            comments: vec![
                IndexedComment {
                    author: "Ada Lovelace".to_owned(),
                    body: "@Elysia this was Sharath".to_owned(),
                },
                IndexedComment {
                    author: "Elysia".to_owned(),
                    body: "Fixed: it reads Sharath now.".to_owned(),
                },
            ],
        }];

        let indexed = document(&segment, &threads, &[]).expect("a document");

        assert_eq!(
            indexed.passages,
            [
                "Launch date\n\nThe launch moved to Friday.\n\n[1:05] Sheriff: Friday works.\n\n\
                 Comment on \"Sheriff: Friday works.\"\nAda Lovelace: @Elysia this was Sharath\n\
                 Elysia: Fixed: it reads Sharath now."
            ]
        );
    }

    #[test]
    fn a_segments_threads_are_indexed_up_to_their_limit() {
        let mut segment = record(SEGMENT_SOURCE_KIND);
        segment.title = String::new();
        segment.body = String::new();
        segment.metadata = json!({ "meetingId": Uuid::now_v7() });
        let long = IndexedThread {
            quote: "Friday".to_owned(),
            comments: vec![IndexedComment {
                author: "Ada".to_owned(),
                body: "word ".repeat(THREADS_MAX_CHARACTERS / 10),
            }],
        };
        let threads = vec![long; 5];

        let indexed = document(&segment, &threads, &[]).expect("a document");
        let indexed_characters: usize = indexed
            .passages
            .iter()
            .map(|passage| passage.chars().count())
            .sum();

        assert!(indexed_characters <= THREADS_MAX_CHARACTERS);
        assert!(indexed_characters > THREADS_MAX_CHARACTERS - passages::PASSAGE_TARGET_CHARACTERS);
    }

    #[test]
    fn a_record_that_says_nothing_has_no_document() {
        let mut empty = record("outlook");
        empty.title = String::new();
        empty.actor = " ".to_owned();
        empty.body = "\n\n".to_owned();
        assert!(document(&empty, &[], &[]).is_none());
    }

    #[test]
    fn a_long_body_is_indexed_up_to_its_limit() {
        let mut segment = record(SEGMENT_SOURCE_KIND);
        segment.title = String::new();
        segment.metadata = json!({ "meetingId": Uuid::now_v7() });
        segment.body = "word ".repeat(BODY_MAX_CHARACTERS); // five times the limit

        let indexed = document(&segment, &[], &[]).expect("a document");
        let indexed_characters: usize = indexed
            .passages
            .iter()
            .map(|passage| passage.chars().count())
            .sum();

        assert!(indexed_characters <= BODY_MAX_CHARACTERS);
        assert!(indexed_characters > BODY_MAX_CHARACTERS - passages::PASSAGE_TARGET_CHARACTERS);
    }

    #[test]
    fn only_confluence_records_are_revisions_of_their_page() {
        let mut page = record("confluence");
        page.external_id = Some("page:42:7".to_owned());
        assert_eq!(
            revision(&page),
            Some(Revision {
                connector_id: page.connector_id.expect("a connector"),
                prefix: "page:42:",
                number: 7,
            })
        );

        let mut orphaned = page.clone();
        orphaned.connector_id = None;
        assert_eq!(revision(&orphaned), None);

        let mut message = record("slack");
        message.external_id = Some("page:42:7".to_owned());
        assert_eq!(revision(&message), None);
    }
}
