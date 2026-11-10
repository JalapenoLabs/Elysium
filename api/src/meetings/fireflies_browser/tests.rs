// Copyright © 2026 Jalapeno Labs

//! What the browser source makes of what Fireflies' web app answers, in the shapes `hive.js`
//! hands back. Every address and name is invented.

use super::*;
use crate::meetings::source::RecordingKind;

fn listed(id: &str, date: &Value, status: &str) -> NoteMeeting {
    serde_json::from_value(json!({
        "id": id,
        "title": format!("Meeting {id}"),
        "date": date,
        "durationMins": 30.5,
        "creator_email": "ada@example.com",
        "allEmails": ["ada@example.com", "grace@example.com"],
        "processMeetingStatus": status,
    }))
    .expect("a listed meeting parses")
}

fn time(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text)
        .expect("a time")
        .with_timezone(&Utc)
}

#[test]
fn a_listing_asks_for_the_meetings_the_person_is_part_of_since_the_cursor_oldest_first() {
    let variables = listing_variables(Some(time("2026-11-01T09:30:00Z")), 50);
    assert_eq!(
        variables,
        json!({
            "from": 0,
            "size": 50,
            "channelId": "USER_PART_OF",
            "filters": { "date": { "from": "2026-11-01T09:30:00.000Z" } },
            "sort": [{ "field": "date", "order": "ASC" }],
        })
    );
    assert_eq!(listing_variables(None, 500)["size"], 150);
    assert_eq!(listing_variables(None, 5)["filters"], json!({}));
}

#[test]
fn a_listed_meeting_reads_its_start_as_milliseconds_or_text_and_links_its_page() {
    let from_milliseconds =
        listed_meeting(listed("01A", &json!(1_762_000_000_000_i64), "completed"))
            .expect("a meeting");
    assert_eq!(
        from_milliseconds.started_at.timestamp_millis(),
        1_762_000_000_000
    );
    assert_eq!(from_milliseconds.duration_ms, Some(1_830_000));
    assert_eq!(from_milliseconds.organizer, "ada@example.com");
    assert_eq!(from_milliseconds.participants.len(), 2);
    assert_eq!(
        from_milliseconds.url.as_deref(),
        Some("https://app.fireflies.ai/view/01A")
    );

    let from_text = listed_meeting(listed(
        "01B",
        &json!("2026-11-01T09:30:00.000Z"),
        "completed",
    ))
    .expect("a meeting");
    assert_eq!(from_text.started_at, time("2026-11-01T09:30:00Z"));

    let as_numeric_text =
        listed_meeting(listed("01C", &json!("1762000000000"), "completed")).expect("a meeting");
    assert_eq!(
        as_numeric_text.started_at.timestamp_millis(),
        1_762_000_000_000
    );

    assert!(listed_meeting(listed("01D", &Value::Null, "completed")).is_none());
}

#[test]
fn participants_sent_as_one_string_are_split() {
    let mut meeting = listed("01A", &json!(1_762_000_000_000_i64), "completed");
    meeting.all_emails = json!("ada@example.com, grace@example.com,");
    let listed = listed_meeting(meeting).expect("a meeting");
    assert_eq!(
        listed.participants,
        ["ada@example.com", "grace@example.com"]
    );
}

#[test]
fn a_listing_stops_at_the_first_meeting_still_being_transcribed_and_answers_newest_first() {
    let now = time("2026-11-10T12:00:00Z");
    let meetings = vec![
        listed("first", &json!("2026-11-10T08:00:00Z"), "completed"),
        listed("second", &json!("2026-11-10T09:00:00Z"), "failed"),
        listed("transcribing", &json!("2026-11-10T10:00:00Z"), "started"),
        listed("after", &json!("2026-11-10T11:00:00Z"), "completed"),
    ];

    let answered: Vec<String> = settled(meetings, now)
        .into_iter()
        .map(|meeting| meeting.external_id)
        .collect();

    // The cursor moves to the newest answered, so the one still transcribing is listed again.
    assert_eq!(answered, ["second", "first"]);
}

#[test]
fn a_meeting_transcribing_past_its_patience_no_longer_holds_the_listing_back() {
    let now = time("2026-11-10T12:00:00Z");
    let meetings = vec![
        listed("stuck", &json!("2026-11-08T10:00:00Z"), "not_started"),
        listed("after", &json!("2026-11-10T11:00:00Z"), "completed"),
    ];

    let answered: Vec<String> = settled(meetings, now)
        .into_iter()
        .map(|meeting| meeting.external_id)
        .collect();

    assert_eq!(answered, ["after", "stuck"]);
}

#[test]
fn only_a_meeting_fireflies_says_is_transcribing_is_unsettled() {
    assert!(!is_settled(Some("not_started")));
    assert!(!is_settled(Some("started")));
    assert!(is_settled(Some("completed")));
    assert!(is_settled(Some("failed")));
    assert!(is_settled(None));
}

fn note(fields: Value) -> MeetingNote {
    serde_json::from_value(fields).expect("a meeting note parses")
}

#[test]
fn captions_become_lines_under_their_speakers_names() {
    let lines = transcript_lines(note(json!({
        "processMeetingStatus": "completed",
        "speakerMeta": { "1": "Ada Lovelace", "2": "  " },
        "captions": [
            { "sentence": " Launch is Friday. ", "speaker_id": 0, "time": 0.48, "endTime": 3.1 },
            { "sentence": "Agreed.", "speaker_id": 1, "time": 3.5, "endTime": 2.0 },
            { "sentence": "Who else?", "speaker_id": 4, "time": 5.0, "endTime": 6.0 },
        ],
    })));

    assert_eq!(
        lines,
        vec![
            TranscriptLine {
                start_ms: 480,
                end_ms: 3100,
                speaker: "Ada Lovelace".to_owned(),
                text: "Launch is Friday.".to_owned(),
            },
            // A blank name, and an end before the start, read as the web app shows them.
            TranscriptLine {
                start_ms: 3500,
                end_ms: 3500,
                speaker: "Speaker 2".to_owned(),
                text: "Agreed.".to_owned(),
            },
            TranscriptLine {
                start_ms: 5000,
                end_ms: 6000,
                speaker: "Speaker 5".to_owned(),
                text: "Who else?".to_owned(),
            },
        ]
    );
}

#[test]
fn the_edits_saved_in_fireflies_are_applied_as_it_shows_them() {
    let lines = transcript_lines(note(json!({
        "speakerMeta": { "1": "Ada", "2": "Grace" },
        "sentenceMeta": { "0": "Launch is on Friday.", "1": "" },
        "labelMeta": { "1": [2] },
        "captions": [
            { "sentence": "Lunch is Friday.", "speaker_id": 0, "time": 1.0, "endTime": 2.0 },
            { "sentence": "Um.", "speaker_id": 0, "time": 2.0, "endTime": 2.5 },
            { "sentence": "Sounds good.", "speaker_id": 0, "time": 3.0, "endTime": 4.0 },
            { "sentence": "...", "speaker_id": 1, "time": 4.0, "endTime": 4.2 },
        ],
    })));

    let read: Vec<(&str, &str)> = lines
        .iter()
        .map(|line| (line.speaker.as_str(), line.text.as_str()))
        .collect();
    // Rewritten, deleted, reassigned to Grace, and a caption without a word hidden.
    assert_eq!(
        read,
        [("Ada", "Launch is on Friday."), ("Grace", "Sounds good.")]
    );
}

#[test]
fn a_caption_without_a_start_is_left_out() {
    let lines = transcript_lines(note(json!({
        "captions": [{ "sentence": "Lost.", "speaker_id": 0, "endTime": 2.0 }],
    })));
    assert!(lines.is_empty());
}

fn result(status: u16, data: Value, errors: Value) -> CallResult {
    let mut fields = json!({ "status": status });
    fields["data"] = data;
    fields["errors"] = errors;
    serde_json::from_value(fields).expect("a result parses")
}

#[test]
fn refusals_are_classified_by_status_and_code() {
    assert!(matches!(
        data_of(result(401, Value::Null, json!([]))),
        Err(SourceError::Unauthorized(_))
    ));
    assert!(matches!(
        data_of(result(
            200,
            Value::Null,
            json!([{ "message": "no session", "code": "UNAUTHENTICATED" }])
        )),
        Err(SourceError::Unauthorized(_))
    ));
    assert!(matches!(
        data_of(result(
            200,
            Value::Null,
            json!([{ "message": "gone", "code": "NOT_FOUND_ERROR" }])
        )),
        Err(SourceError::NotFound)
    ));
    assert!(matches!(
        data_of(result(200, Value::Null, json!([{ "message": "slow down", "code": null }]))),
        Err(SourceError::Refused(message)) if message.contains("slow down")
    ));
    assert!(matches!(
        data_of(result(502, Value::Null, json!([]))),
        Err(SourceError::Refused(message)) if message.contains("502")
    ));
    assert_eq!(
        data_of(result(200, json!({ "ok": true }), json!([]))).expect("data"),
        json!({ "ok": true })
    );
}

#[test]
fn a_refused_recording_costs_only_the_recording() {
    let refused = result(
        200,
        Value::Null,
        json!([{ "message": "upgrade to download", "code": "FORBIDDEN" }]),
    );
    assert!(recording_from(refused, "01A").is_none());

    let offered = result(
        200,
        json!({ "meetingNote": {
            "video_url": "https://media.example.com/v.mp4?signature=redacted",
            "audio_url": "https://media.example.com/a.mp3?signature=redacted",
        } }),
        json!([]),
    );
    assert_eq!(
        recording_from(offered, "01A").map(|recording| recording.kind),
        Some(RecordingKind::Video)
    );

    let unsafe_address = result(
        200,
        json!({ "meetingNote": { "video_url": "http://10.0.0.5/v.mp4", "audio_url": null } }),
        json!([]),
    );
    assert!(recording_from(unsafe_address, "01A").is_none());
}

#[test]
fn the_page_answers_whether_it_is_signed_in_and_one_result_per_call() {
    let answer: Answer = serde_json::from_value(json!({
        "signedIn": true,
        "results": [{ "status": 200, "data": { "meetingNote": null }, "errors": [] }],
    }))
    .expect("the page's answer parses");
    assert!(answer.signed_in);
    assert_eq!(answer.results.len(), 1);

    let signed_out: Answer =
        serde_json::from_value(json!({ "signedIn": false, "results": [] })).expect("parses");
    assert!(!signed_out.signed_in);
}

#[test]
fn the_request_names_each_query_and_its_variables() {
    let calls = [Call {
        query: RECORDING_QUERY,
        variables: json!({ "meetingNoteId": "01A" }),
    }];
    let request = serde_json::to_value(Request { calls: &calls }).expect("serializes");
    assert_eq!(request["calls"][0]["query"], RECORDING_QUERY);
    assert_eq!(request["calls"][0]["variables"]["meetingNoteId"], "01A");
}

#[test]
fn every_query_is_a_query_and_the_script_has_one_place_for_its_request() {
    for query in QUERIES {
        assert!(query.starts_with("query "), "{query}");
        assert!(!query.contains("mutation"), "{query}");
    }
    assert_eq!(SCRIPT.matches(REQUEST_PLACEHOLDER).count(), 1);
}
