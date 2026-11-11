// Copyright © 2026 Jalapeno Labs

//! Slack huddles as meetings, against answers shaped as Slack's web client gets them (ids and
//! words made up, shapes as a signed-in Slack answered them).

use serde_json::json;

use super::*;

fn team() -> Team {
    Team {
        id: "T1".to_owned(),
        name: "Mooreslab".to_owned(),
        url: "https://mooreslab.slack.com/".to_owned(),
        user_id: "U0ME".to_owned(),
        counts: None,
    }
}

/// A huddle as `huddles.history` lists it, ended with its transcript.
fn entry(id: &str, start: i64, end: i64) -> Value {
    json!({
        "id": id,
        "name": "",
        "created_by": "U0ADA",
        "date_start": start,
        "date_end": end,
        "participants": [],
        "participant_history": ["U0ADA", "U0ME"],
        "channels": ["C0STANDUP"],
        "channel_type": "private",
        "has_ended": true,
        "thread_root_ts": format!("{start}.301809"),
        "transcript_file_id": "F0NOTES",
        "attached_file_ids": [],
        "recording": {
            "transcript": true,
            "summary": true,
            "recording_type": "ai_notes_and_transcription",
        },
    })
}

fn at(seconds: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(seconds, 0).expect("a time")
}

fn everything() -> Content {
    Content::EVERYTHING
}

fn names(pairs: &[(&str, &str)]) -> State {
    State {
        names: pairs
            .iter()
            .map(|(id, name)| (format!("T1/{id}"), (*name).to_owned()))
            .collect(),
        ..State::default()
    }
}

#[test]
fn a_huddle_that_ended_with_a_transcript_is_a_meeting() {
    let huddle = huddle_of("T1", &entry("R1", 1_000, 2_800)).expect("a meeting");
    assert_eq!(huddle.id, "R1");
    assert_eq!(huddle.channel.as_deref(), Some("C0STANDUP"));
    assert_eq!(huddle.kind, ConversationKind::Channel);
    assert_eq!(huddle.started_at, at(1_000));
    assert_eq!(huddle.ended_at, at(2_800));
    assert_eq!(huddle.participants, ["U0ADA", "U0ME"]);
    assert_eq!(huddle.transcript_file_id, "F0NOTES");
}

#[test]
fn a_huddle_still_going_or_never_transcribed_is_not_a_meeting() {
    let mut going = entry("R1", 1_000, 0);
    going["has_ended"] = json!(false);
    assert_eq!(huddle_of("T1", &going), None);

    let mut untranscribed = entry("R2", 1_000, 2_800);
    untranscribed["transcript_file_id"] = json!("");
    assert_eq!(huddle_of("T1", &untranscribed), None);
    untranscribed
        .as_object_mut()
        .expect("an object")
        .remove("transcript_file_id");
    assert_eq!(huddle_of("T1", &untranscribed), None);
}

#[test]
fn a_direct_message_huddle_is_known_by_its_conversation_type() {
    let mut direct = entry("R1", 1_000, 2_800);
    direct["channel_type"] = json!("im");
    direct["channels"] = json!(["D0ADA"]);
    assert_eq!(
        huddle_of("T1", &direct).expect("a meeting").kind,
        ConversationKind::Direct
    );
    direct["channel_type"] = json!("mpim");
    assert_eq!(
        huddle_of("T1", &direct).expect("a meeting").kind,
        ConversationKind::Group
    );
}

#[test]
fn a_listing_takes_what_ended_since_newest_first_and_at_most_its_limit() {
    let huddles: Vec<Huddle> = [
        entry("R1", 1_000, 1_500),
        entry("R2", 2_000, 2_500),
        // Started before R2, ended after the cursor: listed, though its start is older.
        entry("R3", 1_800, 3_100),
        entry("R4", 3_000, 3_500),
    ]
    .iter()
    .filter_map(|entry| huddle_of("T1", entry))
    .collect();

    let ids = |chosen: Vec<Huddle>| -> Vec<String> {
        chosen.into_iter().map(|huddle| huddle.id).collect()
    };
    assert_eq!(
        ids(chosen(huddles.clone(), &everything(), Some(at(2_600)), 50)),
        ["R4", "R3"]
    );
    assert_eq!(
        ids(chosen(huddles.clone(), &everything(), None, 2)),
        ["R4", "R2"]
    );
}

#[test]
fn a_listing_reads_only_the_conversations_the_connector_picks() {
    let mut elsewhere = entry("R2", 2_000, 2_500);
    elsewhere["channels"] = json!(["C0OTHER"]);
    let huddles: Vec<Huddle> = [entry("R1", 1_000, 1_500), elsewhere]
        .iter()
        .filter_map(|entry| huddle_of("T1", entry))
        .collect();
    let picked =
        crate::connectors::slack::parse_content(&json!({ "conversations": ["T1/C0STANDUP"] }))
            .expect("content that picks one channel");
    let chosen = chosen(huddles, &picked, None, 50);
    assert_eq!(chosen.len(), 1);
    assert_eq!(chosen[0].id, "R1");
}

#[test]
fn a_listed_huddle_is_named_for_its_channel_and_links_to_its_thread() {
    let huddle = huddle_of("T1", &entry("R1", 1_000, 2_800)).expect("a meeting");
    let names = names(&[("U0ADA", "Ada"), ("U0ME", "Alex"), ("C0STANDUP", "standup")]);
    let listed = listed_meeting(&huddle, &team(), &names);
    assert_eq!(
        listed,
        ListedMeeting {
            external_id: "T1/R1".to_owned(),
            title: "Huddle in #standup".to_owned(),
            started_at: at(1_000),
            duration_ms: Some(1_800_000),
            organizer: "Ada".to_owned(),
            participants: vec!["Ada".to_owned(), "Alex".to_owned()],
            url: Some("https://mooreslab.slack.com/archives/C0STANDUP/p1000301809".to_owned()),
        }
    );
}

#[test]
fn a_direct_huddle_is_named_for_who_joined_and_unknown_names_stay_ids() {
    let mut direct = entry("R1", 1_000, 2_800);
    direct["channel_type"] = json!("im");
    direct["channels"] = json!(["D0ADA"]);
    let huddle = huddle_of("T1", &direct).expect("a meeting");
    let listed = listed_meeting(&huddle, &team(), &names(&[("U0ADA", "Ada")]));
    assert_eq!(listed.title, "Huddle with Ada, U0ME");
}

/// A huddle transcript as `files.info` with `include_transcription` answers it.
fn transcription() -> Value {
    json!({
        "date_start": 1_000,
        "date_end": 1_090,
        "channel_id": "C0STANDUP",
        "transcription_time_ranges": [{ "start_ts": "1000.000000", "stop_ts": "1090.000000" }],
        "blocks": { "type": "rich_text", "block_id": "abc", "elements": [] },
        "lines": [
            { "line_id": "b", "user_id": "U0ME", "start_time_ms": 24_000, "contents": "Launch is Friday." },
            { "line_id": "a", "user_id": "U0ADA", "start_time_ms": 23_000, "contents": " Morning. " },
            { "line_id": "c", "user_id": "U0ADA", "start_time_ms": 53_000, "contents": "   " },
            { "line_id": "d", "user_id": "U0NEW", "start_time_ms": 60_000, "contents": "Sounds good." },
        ],
    })
}

#[test]
fn a_transcript_runs_each_line_to_the_next_and_the_last_to_the_huddles_end() {
    let lines = transcript_of(
        "T1",
        &transcription(),
        &names(&[("U0ADA", "Ada"), ("U0ME", "Alex")]),
    );
    assert_eq!(
        lines,
        [
            TranscriptLine {
                start_ms: 23_000,
                end_ms: 24_000,
                speaker: "Ada".to_owned(),
                text: "Morning.".to_owned(),
            },
            TranscriptLine {
                start_ms: 24_000,
                end_ms: 60_000,
                speaker: "Alex".to_owned(),
                text: "Launch is Friday.".to_owned(),
            },
            // A speaker whose name was not found keeps their id.
            TranscriptLine {
                start_ms: 60_000,
                end_ms: 90_000,
                speaker: "U0NEW".to_owned(),
                text: "Sounds good.".to_owned(),
            },
        ]
    );
}

#[test]
fn slacks_errors_say_whether_to_sign_in_again_or_the_huddle_is_gone() {
    answered("files.info", &json!({ "ok": true })).expect("an answer that is ok");
    assert!(matches!(
        answered(
            "huddles.get",
            &json!({ "ok": false, "error": "invalid_auth" })
        ),
        Err(SourceError::Unauthorized(_))
    ));
    assert!(matches!(
        answered(
            "files.info",
            &json!({ "ok": false, "error": "file_not_found" })
        ),
        Err(SourceError::NotFound)
    ));
    assert!(matches!(
        answered("huddles.history", &json!({ "ok": false, "error": "ratelimited" })),
        Err(SourceError::Refused(message)) if message == "Slack answered huddles.history with ratelimited"
    ));
}
