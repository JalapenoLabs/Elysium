// Copyright © 2026 Jalapeno Labs

//! `elysium_meeting_frames`: the stills of one meeting's video, for its segmenting turn and the
//! subagents it hands them to. See `docs/meetings.md`, Attachments and frames.
//!
//! A grant for it names the meeting, the capture whose recording the stills come from, and the
//! stills the turn was offered (`super::SegmentingScope`), so a turn sees that video alone. Each
//! call checks in the database that the capture is still the meeting's, then takes the still in
//! the workspace container, where the recording is while the meeting is segmented
//! (`crate::meetings::frames::still`), and answers it as an image the model sees.
//!
//! Logs name each call by its meeting and moment, never by what the picture shows.

use futures_util::FutureExt as _;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    CallScope, Instructions, SegmentingScope, Tool, ToolContext, ToolError, ToolServer, internal,
    parse_arguments, with_image,
};
use crate::meetings::frames;
use crate::meetings::segmentation::{clock, moment_of};
use crate::models::meeting_capture;

pub const SERVER: ToolServer = ToolServer {
    name: "elysium_meeting_frames",
    instructions: Instructions::Fixed(
        "Stills of the meeting's video you are segmenting. meeting_frame shows one: a still \
        the prompt names (\"f3\"), or any moment of the meeting by its time on the meeting's \
        clock (\"12:04\"). Look at the stills you may pin, and hand them to subagents to look at \
        when there are several.",
    ),
    tools: &TOOLS,
};

const TOOLS: [Tool; 1] = [Tool {
    name: "meeting_frame",
    description: "Shows the meeting's video at one moment, as an image: a still the prompt names \
        by its name (\"f3\"), or any moment by its time on the meeting's clock, as m:ss or \
        h:mm:ss (\"12:04\"). Answers the moment it shows, with the picture.",
    input_schema: frame_schema,
    run: |context, scope, arguments| run_frame(context, scope, arguments).boxed(),
}];

fn frame_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "frame": {
                "type": "string",
                "minLength": 1,
                "maxLength": 20,
                "description": "A still's name from the prompt, such as f3, or a moment on the \
                    meeting's clock as m:ss or h:mm:ss, such as 12:04.",
            },
        },
        "required": ["frame"],
        "additionalProperties": false,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct FrameArguments {
    frame: String,
}

async fn run_frame(
    context: &ToolContext,
    scope: &CallScope,
    arguments: &str,
) -> Result<Value, ToolError> {
    let segmenting = scope.segmenting()?;
    let arguments: FrameArguments = parse_arguments(arguments)?;
    let capture_ms = capture_ms_of(segmenting, &arguments.frame)?;

    let mut connection = context
        .database
        .get()
        .await
        .map_err(|error| internal(scope, "database.connect", &error))?;
    let capture = meeting_capture::find(&mut connection, segmenting.capture_id)
        .await
        .map_err(|error| match error {
            diesel::result::Error::NotFound => {
                ToolError::NotFound("the meeting's video is gone".to_owned())
            }
            other => internal(scope, "frames.capture", &other),
        })?;
    drop(connection);
    if capture.meeting_id != segmenting.meeting_id {
        return Err(ToolError::NotFound(
            "the video is no longer this meeting's; answer without frames".to_owned(),
        ));
    }

    let still = frames::still(
        &context.workspace,
        segmenting.meeting_id,
        &segmenting.recording_name,
        capture_ms,
    )
    .await
    .map_err(|error| {
        ToolError::NotFound(format!(
            "the video shows nothing at {}: {error}",
            clock(capture_ms + segmenting.offset_ms)
        ))
    })?;
    tracing::event!(
        name: "tools.frames.shown",
        tracing::Level::DEBUG,
        tool.caller = scope.label(),
        meeting.capture_ms = capture_ms,
        "showed a still of a meeting's video",
    );
    Ok(with_image(
        json!({ "moment": clock(capture_ms + segmenting.offset_ms) }),
        "image/jpeg",
        &still,
    ))
}

/// The moment of `segmenting`'s capture that `named` asks for: a still the prompt named, or a
/// moment on the meeting's clock inside the video.
fn capture_ms_of(segmenting: &SegmentingScope, named: &str) -> Result<i64, ToolError> {
    let named = named.trim();
    let capture_ms = match segmenting
        .candidates
        .iter()
        .find(|candidate| candidate.id == named)
    {
        Some(candidate) => candidate.capture_ms,
        None => {
            let meeting_ms = moment_of(named).ok_or_else(|| {
                ToolError::Invalid(format!(
                    "{named} is neither a still the prompt names nor a moment such as 12:04"
                ))
            })?;
            meeting_ms - segmenting.offset_ms
        }
    };
    let inside = capture_ms >= 0
        && segmenting
            .duration_ms
            .is_none_or(|duration_ms| capture_ms <= duration_ms);
    if !inside {
        return Err(ToolError::Invalid(format!(
            "the video runs from {} to {}; ask for a moment inside it",
            clock(segmenting.offset_ms),
            segmenting.duration_ms.map_or_else(
                || "its end".to_owned(),
                |duration_ms| clock(segmenting.offset_ms + duration_ms)
            ),
        )));
    }
    Ok(capture_ms)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use crate::meetings::frames::Candidate;
    use crate::tools::{ElysiaScope, ToolOutput};

    fn segmenting() -> SegmentingScope {
        SegmentingScope {
            meeting_id: Uuid::now_v7(),
            capture_id: Uuid::now_v7(),
            recording_name: "capture.mp4".to_owned(),
            offset_ms: 60_000,
            duration_ms: Some(120_000),
            candidates: vec![Candidate {
                id: "f1".to_owned(),
                capture_ms: 30_000,
                change: 0.5,
            }],
        }
    }

    #[test]
    fn an_image_answer_shows_its_picture_beside_its_text() {
        let output = ToolOutput::from(Ok(with_image(
            json!({ "moment": "1:30" }),
            "image/jpeg",
            b"\xff\xd8\xff",
        )));

        assert_eq!(output.text, "{\"moment\":\"1:30\"}");
        let image = output.image.expect("an image");
        assert_eq!(image.mime_type, "image/jpeg");
        assert_eq!(image.data, "/9j/");
    }

    #[test]
    fn a_still_is_named_or_asked_for_by_its_moment_on_the_meetings_clock() {
        let segmenting = segmenting();
        assert_eq!(capture_ms_of(&segmenting, "f1").expect("offered"), 30_000);
        // The video starts a minute into the meeting, so 1:30 is 30 seconds into it.
        assert_eq!(capture_ms_of(&segmenting, " 1:30 ").expect("inside"), 30_000);
        assert_eq!(capture_ms_of(&segmenting, "0:03:00").expect("its end"), 120_000);
    }

    #[test]
    fn a_moment_outside_the_video_or_a_name_never_offered_is_refused() {
        let segmenting = segmenting();
        for (frame, expected) in [
            ("0:30", "runs from 1:00 to 3:00"),
            ("3:01", "runs from 1:00 to 3:00"),
            ("f9", "neither a still"),
            ("soon", "neither a still"),
        ] {
            let refused = capture_ms_of(&segmenting, frame).expect_err(frame);
            assert!(refused.to_string().contains(expected), "{frame}: {refused}");
        }
    }

    #[test]
    fn only_a_segmenting_turn_reaches_the_frames() {
        let elysia = CallScope::Elysia(ElysiaScope {
            turn_id: Uuid::now_v7(),
        });
        let refused = elysia.segmenting().expect_err("Elysia's own turn");
        assert!(refused.to_string().contains("a meeting's segmenting"));
        assert!(CallScope::Segmenting(segmenting()).segmenting().is_ok());
    }
}
