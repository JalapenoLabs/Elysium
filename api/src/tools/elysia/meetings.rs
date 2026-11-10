// Copyright © 2026 Jalapeno Labs

//! `elysia_meeting_edit` and `elysia_meeting_add_to_project`: what Elysia does when a person asks
//! her in a comment thread on a meeting ("this was Sharath", "this belongs to the launch"). See
//! `crate::meetings::comments` and `docs/elysia.md`, Tools.
//!
//! Both act only for a turn a thread started, and only on that thread's meeting: the turn's
//! trigger names it, so a turn about anything else is refused. Each does what a person does on the
//! meeting's page, as hers. An edit replaces words exactly as a person's does
//! (`meetings::comments::edit_words`), in the meeting's history with Undo. Putting the range on a
//! project attributes the topics it lies in, each a segment's timeline record, as her own
//! attribution (`models::attribution`), which never overrides a person's.

use chrono::Utc;
use diesel_async::AsyncPgConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::elysia::thread::Asked;
use crate::elysia::trigger;
use crate::meetings::comments;
use crate::models::agent::Author;
use crate::models::attribution::{self, Attributor, Written};
use crate::models::meeting::{self, Meeting};
use crate::models::meeting_mark::{self, Mark, Range};
use crate::models::meeting_word::{EDIT_TEXT_MAX_CHARACTERS, EditError, Replacement};
use crate::models::user::ELYSIA_USER_ID;
use crate::models::{elysia, project};
use crate::realtime::ServerEvent;
use crate::tools::{CallScope, ToolContext, ToolError, internal, parse_arguments};

/// How sure her attribution is when a person told her, in a thread, what the range is about. A
/// person's own is certain (1); hers stays a little below it, so it still reads as hers, which
/// the person confirms with one click.
const ASKED_CONFIDENCE: f32 = 0.9;

pub fn edit_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "startWordId": {
                "type": "string",
                "format": "uuid",
                "description": "The first word to replace, by the id the thread's prompt gives it.",
            },
            "endWordId": {
                "type": "string",
                "format": "uuid",
                "description": "The last word to replace; the same as startWordId for one word.",
            },
            "text": {
                "type": "string",
                "description": "What the words say instead, split into words at its spaces.",
                "maxLength": EDIT_TEXT_MAX_CHARACTERS,
            },
        },
        "required": ["startWordId", "endWordId", "text"],
        "additionalProperties": false,
    })
}

pub fn add_to_project_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "projectId": {
                "type": "string",
                "format": "uuid",
                "description": "The project, from elysia_overview.",
            },
        },
        "required": ["projectId"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct EditArguments {
    start_word_id: Uuid,
    end_word_id: Uuid,
    text: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AddToProjectArguments {
    project_id: Uuid,
}

pub async fn run_edit(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let turn_id = scope.elysia()?.turn_id;
    let arguments: EditArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let (found, _) = asked_thread(&mut connection, scope, turn_id).await?;
    let replacement = Replacement {
        range: Range {
            start_word_id: arguments.start_word_id,
            end_word_id: arguments.end_word_id,
        },
        text: &arguments.text,
    };
    let entry = comments::edit_words(&mut connection, &found, replacement, elysia_now())
        .await
        .map_err(|error| match error {
            EditError::NotFound => ToolError::Invalid(
                "a word named is not this meeting's; name words by the ids the thread's prompt \
                 gives"
                    .to_owned(),
            ),
            EditError::NotStanding => ToolError::Invalid(
                "a word of that range was changed meanwhile; nothing was edited".to_owned(),
            ),
            EditError::OutOfOrder => {
                ToolError::Invalid("the range ends before it starts".to_owned())
            }
            EditError::Empty => ToolError::Invalid("an edit writes at least one word".to_owned()),
            EditError::TooLong => ToolError::Invalid(format!(
                "an edit writes at most {EDIT_TEXT_MAX_CHARACTERS} characters"
            )),
            EditError::Database(error) => internal(scope, "elysia.meeting_edit", &error),
        })?;
    context.events.publish(&ServerEvent::MeetingChanged {
        id: found.id,
        state: found.state.clone(),
        created_by: found.created_by,
    });
    Ok(json!({
        "note": "Edited. The meeting's history has it as yours, with Undo.",
        "before": entry.data["beforeText"],
        "after": entry.data["afterText"],
    }))
}

pub async fn run_add_to_project(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let turn_id = scope.elysia()?.turn_id;
    let arguments: AddToProjectArguments = parse_arguments(arguments)?;
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let (found, mark) = asked_thread(&mut connection, scope, turn_id).await?;
    match project::find(&mut connection, arguments.project_id).await {
        Ok(_) => {}
        Err(diesel::result::Error::NotFound) => {
            return Err(ToolError::Invalid(
                "no such project; call elysia_overview for the projects".to_owned(),
            ));
        }
        Err(error) => return Err(internal(scope, "elysia.meeting_add_to_project", &error)),
    }
    let span = comments::span_of(&mut connection, &mark)
        .await
        .map_err(|error| internal(scope, "elysia.meeting_add_to_project", &error))?;
    let topics = comments::segments_over(&mut connection, &found, span)
        .await
        .map_err(|error| internal(scope, "elysia.meeting_add_to_project", &error))?;
    if topics.is_empty() {
        return Err(ToolError::Invalid(
            "the range lies in none of the meeting's topics yet, so there is nothing to put on \
             a project until it is segmented"
                .to_owned(),
        ));
    }

    let mut answered = Vec::with_capacity(topics.len());
    for (record, _) in topics {
        let written = attribution::attribute(
            &mut connection,
            record.id,
            arguments.project_id,
            ASKED_CONFIDENCE,
            Attributor::Elysia,
        )
        .await
        .map_err(|error| internal(scope, "elysia.meeting_add_to_project", &error))?;
        let outcome = match written {
            Written::Changed(_) => {
                context.events.publish(&ServerEvent::TimelineRecordChanged {
                    id: record.id,
                    created_by: record.created_by,
                });
                "added"
            }
            Written::Unchanged => "already on it",
            Written::PersonDecided => "left as a person decided it",
        };
        answered.push(json!({ "topic": record.title, "outcome": outcome }));
    }
    Ok(json!({ "topics": answered }))
}

/// The meeting and thread turn `turn_id` was started by, standing, or a refusal saying that this
/// turn was started by no thread.
async fn asked_thread(
    connection: &mut AsyncPgConnection,
    scope: &CallScope,
    turn_id: Uuid,
) -> Result<(Meeting, Mark), ToolError> {
    let turn = elysia::find_turn(connection, turn_id)
        .await
        .map_err(|error| internal(scope, "elysia.meeting_thread", &error))?;
    let asked = (turn.trigger_kind == trigger::MEETING_COMMENT)
        .then(|| Asked::of(&turn.trigger_detail))
        .flatten()
        .ok_or_else(|| {
            ToolError::Invalid(
                "the meeting tools act only on the thread a person asked you in, and this turn \
                 was started by no thread"
                    .to_owned(),
            )
        })?;
    let gone = || ToolError::NotFound("the thread, or its meeting, is gone".to_owned());
    let found = match meeting::find(connection, asked.meeting).await {
        Ok(found) => found,
        Err(diesel::result::Error::NotFound) => return Err(gone()),
        Err(error) => return Err(internal(scope, "elysia.meeting_thread", &error)),
    };
    let mark = match meeting_mark::find(connection, found.id, asked.thread).await {
        Ok(mark) if mark.removed_by.is_none() => mark,
        Ok(_) | Err(diesel::result::Error::NotFound) => return Err(gone()),
        Err(error) => return Err(internal(scope, "elysia.meeting_thread", &error)),
    };
    Ok((found, mark))
}

/// Elysia, acting now.
fn elysia_now() -> Author<'static> {
    Author {
        user_id: ELYSIA_USER_ID,
        agent: None,
        at: Utc::now(),
    }
}
