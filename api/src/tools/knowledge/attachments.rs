// Copyright © 2026 Jalapeno Labs

//! `meeting_attachment_get`: one file or frame of a meeting, for the agent an excerpt it is pinned
//! in reached. See `docs/meetings.md`, Attachments and frames.
//!
//! An excerpt carries the files and frames pinned over it to its projects, so a Project Manager,
//! a worker on a batch it called for, and any agent reading it in the knowledge base are told of
//! them by id (`crate::meetings::attachments::agent_view`). This opens one: a frame, or a file that
//! is a proven image, as the picture the agent sees; any other file as the text read from it.
//!
//! Each call checks, in the database, that the turn reads for someone who may read its meeting
//! (`docs/meeting-review.md`, Who sees a meeting), as only they find the excerpts it is pinned in.
//! Anything else answers as if there were no such attachment. The turn keeps the meeting as one it
//! read before it is answered, so what it writes is held to it (`models::meeting_reads`).

use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::reader;
use crate::meeting_attachments;
use crate::meetings::attachments::agent_view;
use crate::models::meeting::{self, Reader};
use crate::models::meeting_attachment::{self, AttachmentKind};
use crate::models::{storage_location, user};
use crate::tools::{
    CallScope, ToolContext, ToolError, internal, keep_read, parse_arguments, with_image,
};

/// The tool's description, which the agent reads.
pub const DESCRIPTION: &str = "Opens one file or frame pinned in a meeting's excerpt, by the \
    attachmentId a record, a batch, or a Project Manager's prompt names: a frame, or a file that \
    is an image, as the picture itself; any other file as the text read from it, its first 40,000 \
    characters.";

/// The most of a file's text one answer holds, in characters: a long document's opening and its
/// gist, and a bound on one answer, as a page's text is.
const TEXT_MAX_CHARACTERS: usize = 40_000;

/// The largest image shown, in bytes: an image past it would cost the model far more than it
/// shows, and a frame is never near it.
const IMAGE_MAX_BYTES: i64 = 5_000_000;

pub fn schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "attachmentId": {
                "type": "string",
                "format": "uuid",
                "description": "The file's or frame's attachmentId.",
            },
        },
        "required": ["attachmentId"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Arguments {
    attachment_id: Uuid,
}

pub async fn run(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let arguments: Arguments = parse_arguments(arguments)?;
    let reader_id = reader(context, scope).await?;
    let unknown = || {
        ToolError::NotFound(format!(
            "no file or frame {} is pinned where you may read it",
            arguments.attachment_id
        ))
    };
    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let attachment =
        match meeting_attachment::find_by_id(&mut connection, arguments.attachment_id).await {
            Ok(found) if found.removed_by.is_none() => found,
            Ok(_) | Err(diesel::result::Error::NotFound) => return Err(unknown()),
            Err(error) => return Err(internal(scope, "knowledge.attachment", &error)),
        };
    // Its meeting's readers alone reach it, as they alone find the excerpts it is pinned in.
    let reading = user::find(&mut connection, reader_id)
        .await
        .map_err(|error| internal(scope, "knowledge.attachment.reader", &error))?;
    match meeting::find_readable(&mut connection, attachment.meeting_id, Reader::of(&reading)).await
    {
        Ok(_readable) => {}
        Err(diesel::result::Error::NotFound) => return Err(unknown()),
        Err(error) => return Err(internal(scope, "knowledge.attachment.meeting", &error)),
    }
    keep_read(&mut connection, scope, &[attachment.meeting_id]).await?;
    let Some(path) = attachment.storage_path.clone() else {
        return Err(ToolError::NotFound(
            "that file is still being stored; open it again in a minute".to_owned(),
        ));
    };

    let mut view = agent_view(&attachment);
    let shows_picture = attachment.kind() == AttachmentKind::Frame
        || meeting_attachments::displays_inline(&attachment.content_type);
    if !shows_picture {
        let text = attachment.text.as_deref().unwrap_or_default();
        let shown: String = text.chars().take(TEXT_MAX_CHARACTERS).collect();
        view["textCut"] = json!(shown.len() < text.len());
        view["text"] = if attachment.text.is_some() {
            json!(shown)
        } else {
            Value::Null
        };
        return Ok(json!({ "attachment": view }));
    }
    if attachment.size_bytes > IMAGE_MAX_BYTES {
        return Err(ToolError::Invalid(format!(
            "that image is larger than {IMAGE_MAX_BYTES} bytes, more than is shown"
        )));
    }
    let location = storage_location::find(&mut connection, attachment.storage_location_id)
        .await
        .map_err(|error| internal(scope, "knowledge.attachment.location", &error))?;
    drop(connection);
    let access_key = location
        .access_key(&context.cipher)
        .map_err(|error| internal(scope, "knowledge.attachment.key", &error))?;
    let download = context
        .storage
        .download(&location, &access_key, &path)
        .await
        .map_err(|error| internal(scope, "knowledge.attachment.download", &error))?;
    let bytes = futures_util::TryStreamExt::try_fold(
        download.body,
        Vec::new(),
        |mut read, chunk| async move {
            read.extend_from_slice(&chunk);
            Ok(read)
        },
    )
    .await
    .map_err(|error| internal(scope, "knowledge.attachment.read", &error))?;
    Ok(with_image(
        json!({ "attachment": view }),
        &attachment.content_type,
        &bytes,
    ))
}
