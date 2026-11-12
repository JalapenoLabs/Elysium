// Copyright © 2026 Jalapeno Labs

//! A comment thread on a meeting that mentioned Elysia (`crate::meetings::comments`): what her
//! turn's prompt carries of it ([`load`]), and her answer, posted in it as hers ([`answer`]).
//!
//! The prompt quotes the range with each word's id, which her meeting tools name words by
//! (`tools::elysia`), a little of what was said around it, the topics it lies in with their
//! projects, and the whole thread. A turn whose thread she already answered after the comment
//! that asked her, or whose thread is gone, has nothing left to do.

use std::collections::HashMap;

use chrono::Utc;
use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use serde_json::Value;
use uuid::Uuid;

use super::prompt::{MeetingThread, PromptMessage, ThreadTopic, ThreadWord};
use crate::meetings::comments::{self, CommentError, Writing};
use crate::models::agent::Author;
use crate::models::meeting_comment::{self, BODY_MAX_CHARACTERS};
use crate::models::user::ELYSIA_USER_ID;
use crate::models::{meeting, meeting_mark, meeting_word, project};
use crate::realtime::EventBus;

/// The most of a thread's range the prompt quotes word by word. A range can be the whole
/// meeting; what a comment is about is nearly always a sentence or two.
const RANGE_WORDS_MAX: usize = 300;

/// How many words around the range the prompt quotes, so she reads it in its sentence.
const CONTEXT_WORDS: usize = 30;

/// The thread a `meeting-comment` turn was queued for, as its detail names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Asked {
    pub meeting: Uuid,
    /// The thread's comment mark.
    pub thread: Uuid,
    /// The comment that mentioned her.
    pub comment: Uuid,
}

impl Asked {
    /// The thread `detail` names, as `meetings::comments` queues it.
    pub fn of(detail: &Value) -> Option<Self> {
        let id = |key: &str| {
            detail
                .get(key)
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<Uuid>().ok())
        };
        Some(Self {
            meeting: id("meetingId")?,
            thread: id("markId")?,
            comment: id("commentId")?,
        })
    }
}

/// What a thread's turn finds.
#[derive(Debug)]
pub enum Loaded {
    /// The thread, for her prompt.
    Thread(MeetingThread),
    /// She answered after the comment that asked her, in an earlier turn.
    Answered,
    /// The thread, or its meeting, is gone.
    Gone,
}

/// The thread `asked` names, as her prompt quotes it, with `names` naming who wrote each comment.
///
/// # Errors
/// Propagates any database error.
pub async fn load(
    connection: &mut AsyncPgConnection,
    asked: Asked,
    names: &HashMap<Uuid, String>,
) -> QueryResult<Loaded> {
    let Some(found) = meeting::find(connection, asked.meeting)
        .await
        .map(Some)
        .or_else(missing)?
    else {
        return Ok(Loaded::Gone);
    };
    let Some(mark) = meeting_mark::find(connection, found.id, asked.thread)
        .await
        .map(Some)
        .or_else(missing)?
    else {
        return Ok(Loaded::Gone);
    };
    if mark.removed_by.is_some() {
        return Ok(Loaded::Gone);
    }
    let thread = meeting_comment::of_thread(connection, mark.id).await?;
    // Comment ids are UUIDv7, so one written after the asking comment sorts after it.
    let answered = thread.iter().any(|written| {
        written.comment.created_by == ELYSIA_USER_ID && written.comment.id > asked.comment
    });
    if answered {
        return Ok(Loaded::Answered);
    }

    let words = meeting_word::standing(connection, found.id).await?;
    let start = words.iter().position(|word| word.id == mark.start_word_id);
    let end = words.iter().position(|word| word.id == mark.end_word_id);
    let (Some(start), Some(end)) = (start, end) else {
        // A mark is anchored to standing words; one that is not is a transcript read mid-change.
        return Ok(Loaded::Gone);
    };
    let quoted_end = end.min(start + RANGE_WORDS_MAX - 1);
    let span = (
        words[start].start_ms,
        words[end].end_ms.max(words[start].start_ms),
    );
    let text_of = |slice: &[meeting_word::Word]| {
        slice
            .iter()
            .map(|word| word.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    };

    let project_names: HashMap<Uuid, String> = project::list(connection)
        .await?
        .into_iter()
        .map(|listed| (listed.id, listed.name))
        .collect();
    let topics = comments::segments_over(connection, &found, span)
        .await?
        .into_iter()
        .map(|(record, attributions)| ThreadTopic {
            title: record.title,
            projects: attributions
                .iter()
                .map(|attribution| {
                    let name = project_names
                        .get(&attribution.project_id)
                        .map_or("A project", String::as_str);
                    format!("{name} ({})", attribution.project_id)
                })
                .collect(),
        })
        .collect();

    Ok(Loaded::Thread(MeetingThread {
        meeting_title: found.title,
        start_ms: span.0,
        end_ms: span.1,
        before: text_of(&words[start.saturating_sub(CONTEXT_WORDS)..start]),
        after: text_of(&words[end + 1..(end + 1 + CONTEXT_WORDS).min(words.len())]),
        words: words[start..=quoted_end]
            .iter()
            .map(|word| ThreadWord {
                id: word.id,
                speaker: word.speaker.clone(),
                text: word.text.clone(),
            })
            .collect(),
        words_left_out: end - quoted_end,
        topics,
        comments: thread
            .into_iter()
            .map(|written| PromptMessage {
                author: names
                    .get(&written.comment.created_by)
                    .cloned()
                    .unwrap_or_else(|| "Someone".to_owned()),
                body: written.comment.body,
                sent_at: written.comment.created_at,
            })
            .collect(),
    }))
}

/// Posts `reply` in the thread `asked` names, as Elysia, and tells every client. A thread
/// removed while she worked takes no answer, which is logged.
///
/// # Errors
/// Propagates any database error.
pub async fn answer(
    connection: &mut AsyncPgConnection,
    events: &EventBus,
    asked: Asked,
    reply: &str,
) -> QueryResult<()> {
    let Some(found) = meeting::find(connection, asked.meeting)
        .await
        .map(Some)
        .or_else(missing)?
    else {
        tracing::event!(
            name: "elysia.thread.gone",
            tracing::Level::WARN,
            meeting.id = %asked.meeting,
            "the meeting Elysia answered a thread of is gone; her answer was not posted",
        );
        return Ok(());
    };
    let body: String = reply.chars().take(BODY_MAX_CHARACTERS).collect();
    let writing = Writing {
        body: &body,
        mentions: &[],
    };
    let author = Author {
        user_id: ELYSIA_USER_ID,
        agent: None,
        at: Utc::now(),
    };
    match comments::reply(connection, &found, asked.thread, writing, author).await {
        Ok(outcome) => {
            outcome
                .announcement
                .publish(events, connection, &found, None)
                .await;
            Ok(())
        }
        Err(CommentError::NotFound | CommentError::Removed) => {
            tracing::event!(
                name: "elysia.thread.gone",
                tracing::Level::WARN,
                meeting.id = %found.id,
                meeting_mark.id = %asked.thread,
                "the thread Elysia answered was removed meanwhile; her answer was not posted",
            );
            Ok(())
        }
        Err(CommentError::Merged) => {
            tracing::event!(
                name: "elysia.thread.merged",
                tracing::Level::WARN,
                meeting.id = %found.id,
                meeting_mark.id = %asked.thread,
                "the meeting Elysia answered in was merged into another meanwhile; her answer was \
                 not posted",
            );
            Ok(())
        }
        Err(CommentError::Database(error)) => Err(error),
        Err(error) => {
            tracing::event!(
                name: "elysia.thread.refused",
                tracing::Level::ERROR,
                meeting.id = %found.id,
                meeting_mark.id = %asked.thread,
                error.message = %error,
                "Elysia's answer in a thread was refused",
            );
            Ok(())
        }
    }
}

/// A row that is not there as `None`, and any other error as itself.
fn missing<Found>(error: diesel::result::Error) -> QueryResult<Option<Found>> {
    match error {
        diesel::result::Error::NotFound => Ok(None),
        error => Err(error),
    }
}
