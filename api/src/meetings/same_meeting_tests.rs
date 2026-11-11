// Copyright © 2026 Jalapeno Labs

//! One meeting recorded twice, end to end against Postgres: Teams and Fireflies each list the same
//! launch review, on clocks four seconds apart, Fireflies mishearing a name and recording on after
//! Teams stopped. Aligning asks before anything merges them, a merge lines both captures up on one
//! clock keeping a person's edit, a split puts everything back, and an offset set by hand moves a
//! capture's words until it is undone. See `docs/meetings.md`, Aligning and The same meeting twice.

use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde_json::json;
use uuid::Uuid;

use super::align::AlignMeeting;
use super::clock;
use super::correct::CorrectTranscript;
use super::duplicates;
use super::history;
use super::source::TranscriptLine;
use super::transcribe::TranscribedMeeting;
use super::workflow;
use crate::database::Pool;
use crate::models::action_item::{self, ActionItemState};
use crate::models::agent::Author;
use crate::models::meeting::{self, Meeting, NewMeeting};
use crate::models::meeting_capture::{
    self, Fetched, MeetingCapture, NewMeetingCapture, OffsetSource,
};
use crate::models::meeting_duplicate::{self, MeetingDuplicate};
use crate::models::meeting_event::{self, EventKind};
use crate::models::meeting_mark::{self, MarkKind, Range};
use crate::models::meeting_transcription::{self, NewMeetingTranscription};
use crate::models::meeting_word::{self, Replacement, Word};
use crate::models::step_job::{self, StepJobState, StepOutcome};
use crate::models::workflow_run::{self, WorkflowRunState};
use crate::realtime::EventBus;
use crate::test_support::{TEST_PERSON_ID, migrated_database};
use crate::whisper::{WhisperSegment, WhisperTranscript, WhisperWord};
use crate::workflows::{self, Done, Step, StepContext};

/// How far Fireflies' clock runs behind Teams': its recording began four seconds into the meeting
/// though both list it as starting on the hour.
const FIREFLIES_LATE_MS: i64 = 4_000;

fn line(start_ms: i64, end_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
    TranscriptLine {
        start_ms,
        end_ms,
        speaker: speaker.to_owned(),
        text: text.to_owned(),
    }
}

/// The launch review as Teams heard it, on the meeting's clock: its transcript names everyone, and
/// it stopped recording after thirty seconds.
fn teams_lines() -> Vec<TranscriptLine> {
    vec![
        line(
            0,
            6_000,
            "Ada",
            "Morning everyone, thanks for joining the launch review.",
        ),
        line(
            6_500,
            14_000,
            "Grace",
            "Sharath confirmed the Elysium migration finishes Thursday evening.",
        ),
        line(
            14_500,
            22_000,
            "Ada",
            "Then we ship the onboarding checklist and the pricing page Friday.",
        ),
        line(
            22_500,
            30_000,
            "Grace",
            "Mooreslab wants the customer interviews scheduled next week.",
        ),
    ]
}

/// The same review as Fireflies heard it, on its own clock, four seconds behind: it misheard a
/// name, timed its lines a little differently, and kept recording until the goodbyes.
fn fireflies_lines() -> Vec<TranscriptLine> {
    let behind =
        |start_ms: i64, end_ms: i64| (start_ms - FIREFLIES_LATE_MS, end_ms - FIREFLIES_LATE_MS);
    let mut lines = Vec::new();
    for (start_ms, end_ms, speaker, text) in [
        (
            4_200,
            6_100,
            "Ada Lovelace",
            "thanks for joining the launch review.",
        ),
        (
            6_600,
            14_100,
            "Grace Hopper",
            "Sheriff confirmed the Elysium migration finishes Thursday evening.",
        ),
        (
            14_400,
            21_900,
            "Ada Lovelace",
            "Then we ship the onboarding checklist and the pricing page Friday.",
        ),
        (
            22_600,
            30_100,
            "Grace Hopper",
            "Moore's lab wants the customer interviews scheduled next week.",
        ),
        (36_000, 38_500, "Grace Hopper", "Demo on Friday."),
        (
            40_000,
            45_000,
            "Ada Lovelace",
            "Thanks all, see you Friday.",
        ),
    ] {
        let (start_ms, end_ms) = behind(start_ms, end_ms);
        lines.push(line(start_ms, end_ms, speaker, text));
    }
    lines
}

/// What Whisper heard of Teams' recording: each line's words, timed exactly, speakers unknown.
fn teams_whisper() -> WhisperTranscript {
    let segments = teams_lines()
        .iter()
        .map(|heard| {
            let texts: Vec<&str> = heard.text.split_whitespace().collect();
            let count = i64::try_from(texts.len()).expect("a few words");
            // Whisper hears each word a little inside its line, as speech rarely fills one.
            let start_ms = heard.start_ms + 150;
            let step = (heard.end_ms - 300 - start_ms) / count;
            let words = texts
                .iter()
                .zip(0_i64..)
                .map(|(text, index)| WhisperWord {
                    start_ms: start_ms + step * index,
                    end_ms: start_ms + step * index + step - 40,
                    text: format!(" {text}"),
                    probability: 0.93,
                })
                .collect();
            WhisperSegment {
                start_ms,
                end_ms: heard.end_ms - 150,
                text: heard.text.clone(),
                words,
            }
        })
        .collect();
    WhisperTranscript {
        model: "large-v3".to_owned(),
        language: "en".to_owned(),
        language_probability: 0.99,
        duration_ms: 30_000,
        segments,
    }
}

/// Stores a meeting of the test person's as `source_kind` listed it, starting `started_at`, with
/// its capture fetched as `lines`, and Whisper's transcript of it when there is one. Teams names
/// who took part; Fireflies gives their addresses, as the two sources do.
async fn listed_and_fetched(
    connection: &mut AsyncPgConnection,
    source_kind: &str,
    started_at: DateTime<Utc>,
    lines: &[TranscriptLine],
    whisper: Option<WhisperTranscript>,
) -> (Meeting, MeetingCapture) {
    let meeting_id = Uuid::now_v7();
    let (listed, capture) = meeting::insert_new(
        connection,
        NewMeeting {
            id: meeting_id,
            title: "Launch review".to_owned(),
            started_at,
            duration_ms: Some(1_800_000),
            organizer: "ada@example.com".to_owned(),
            participants: if source_kind == "teams" {
                json!(["Ada Lovelace", "Grace Hopper"])
            } else {
                json!(["ada.lovelace@example.com", "grace@example.com"])
            },
            created_by: TEST_PERSON_ID,
        },
        NewMeetingCapture {
            id: Uuid::now_v7(),
            meeting_id,
            connector_id: None,
            source_kind: source_kind.to_owned(),
            external_id: Some(Uuid::now_v7().to_string()),
            url: None,
            created_by: TEST_PERSON_ID,
        },
    )
    .await
    .expect("stores")
    .expect("new");
    let capture = meeting_capture::record_fetched(
        connection,
        capture.id,
        Fetched {
            transcript: serde_json::to_value(lines).expect("serializes"),
            transcript_hash: super::fetch::transcript_hash(lines),
            recording: None,
        },
    )
    .await
    .expect("records")
    .expect("the first fetch");
    if let Some(whisper) = whisper {
        meeting_transcription::record(
            connection,
            NewMeetingTranscription {
                capture_id: capture.id,
                transcript_hash: super::transcribe::whisper_transcript_hash(&whisper),
                transcript: serde_json::to_value(&whisper).expect("serializes"),
            },
        )
        .await
        .expect("records")
        .expect("new");
    }
    let listed = meeting::record_recorded(connection, listed.id)
        .await
        .expect("moves");
    (listed, capture)
}

fn person() -> Author<'static> {
    Author {
        user_id: TEST_PERSON_ID,
        agent: None,
        at: Utc::now(),
    }
}

async fn standing(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> Vec<Word> {
    meeting_word::standing(connection, meeting_id)
        .await
        .expect("reads")
}

fn text(words: &[Word]) -> String {
    meeting_word::text_of(words)
}

/// Runs meeting `fetched`'s alignment the way the engine does, on the input its run would hand it.
async fn align(
    database: &Pool,
    connection: &mut AsyncPgConnection,
    fetched: &Meeting,
) -> Done<TranscribedMeeting> {
    let run = workflow::run_alignment(connection, fetched)
        .await
        .expect("builds");
    let input: TranscribedMeeting = serde_json::from_value(run.input).expect("reads");
    AlignMeeting::for_test(database.clone())
        .run(
            input,
            StepContext::for_test::<AlignMeeting>(database.clone()),
        )
        .await
        .expect("an attempt")
}

/// Starts `fetched`'s run of the meeting workflow at aligning, and runs that step through the
/// queue as the engine's workers would, recording how it ended.
async fn align_through_the_queue(
    database: &Pool,
    connection: &mut AsyncPgConnection,
    fetched: &Meeting,
) -> (Uuid, Done<TranscribedMeeting>) {
    let events = EventBus::in_process();
    let new_run = workflow::run_from_aligning(connection, fetched)
        .await
        .expect("builds");
    let run_id = workflows::start(connection, &events, new_run)
        .await
        .expect("starts")
        .settled
        .run
        .id;
    let at = Utc::now();
    let claims = step_job::claim(connection, at, 1)
        .await
        .expect("claims")
        .claims;
    let claim = claims.first().expect("the align step is due");
    assert_eq!(claim.job.step, AlignMeeting::NAME);
    let input = serde_json::from_value(claim.job.input.clone().expect("an input")).expect("reads");
    let done = AlignMeeting::for_test(database.clone())
        .run(
            input,
            StepContext::for_test::<AlignMeeting>(database.clone()),
        )
        .await
        .expect("an attempt");
    let outcome = match &done {
        Done::Succeeded { output, summary } => StepOutcome::Succeeded {
            output: serde_json::to_value(output).expect("serializes"),
            summary: summary.clone(),
        },
        Done::Held { output, summary } => StepOutcome::Held {
            output: serde_json::to_value(output).expect("serializes"),
            summary: summary.clone(),
        },
        Done::Skipped { reason } => StepOutcome::Skipped {
            reason: reason.clone(),
        },
    };
    step_job::finish(connection, claim, outcome, at)
        .await
        .expect("records")
        .expect("the claim holds");
    (run_id, done)
}

async fn pair_of(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> Vec<MeetingDuplicate> {
    meeting_duplicate::of_meeting(connection, meeting_id)
        .await
        .expect("reads")
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one meeting recorded twice, from listing to merge, split, and merge again"
)]
async fn one_meeting_recorded_twice_is_asked_about_merged_on_one_clock_and_split_back() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let on_the_hour: DateTime<Utc> = "2026-11-10T15:00:00Z".parse().expect("a time");

    // Teams lists the review first, and aligning it finds nothing else.
    let (teams, teams_capture) = listed_and_fetched(
        &mut connection,
        "teams",
        on_the_hour,
        &teams_lines(),
        Some(teams_whisper()),
    )
    .await;
    let aligned = align(&database, &mut connection, &teams).await;
    assert!(matches!(aligned, Done::Succeeded { .. }), "{aligned:?}");
    let teams_words = standing(&mut connection, teams.id).await;
    assert!(text(&teams_words).starts_with("Morning everyone, thanks for joining"));
    // Whisper timed the words where it heard them; the source named who spoke.
    let sharath = teams_words
        .iter()
        .find(|word| word.text == "Sharath")
        .expect("the source's spelling");
    assert_eq!(sharath.speaker, "Grace");
    let confirmed = teams_words
        .iter()
        .find(|word| word.text == "confirmed")
        .expect("a word Whisper heard too");
    assert_eq!(
        confirmed.start_ms,
        teams_whisper().segments[1].words[1].start_ms
    );

    // Ada corrects a word and takes a sentence into a project before Fireflies shows up.
    let evening = teams_words
        .iter()
        .find(|word| word.text == "evening.")
        .expect("a word")
        .clone();
    meeting_word::edit(
        &mut connection,
        teams.id,
        Replacement {
            range: Range {
                start_word_id: evening.id,
                end_word_id: evening.id,
            },
            text: "night.",
        },
        person(),
    )
    .await
    .expect("edits");
    let words = standing(&mut connection, teams.id).await;
    let mooreslab = words
        .iter()
        .find(|word| word.text == "Mooreslab")
        .expect("a word")
        .clone();
    let week = words
        .iter()
        .find(|word| word.text == "week.")
        .expect("a word")
        .clone();
    let (excerpt, _entry) = meeting_mark::add(
        &mut connection,
        teams.id,
        MarkKind::Excerpt,
        Range {
            start_word_id: mooreslab.id,
            end_word_id: week.id,
        },
        json!({}),
        person(),
    )
    .await
    .expect("marks");

    // Fireflies lists the same review a moment later, starting on the hour as the calendar has it,
    // though its recording began four seconds in. Aligning it asks before anything merges.
    let (fireflies, fireflies_capture) = listed_and_fetched(
        &mut connection,
        "fireflies",
        on_the_hour,
        &fireflies_lines(),
        None,
    )
    .await;
    let (fireflies_run, held) =
        align_through_the_queue(&database, &mut connection, &fireflies).await;
    assert!(matches!(held, Done::Held { .. }), "{held:?}");
    // Ada fixes a word only Fireflies heard, the goodbye's last, while it waits: the edit must
    // travel with its capture when the merge and then aligning move that capture onto Teams'
    // clock, and the same word said a moment before it must stay as it was said.
    let goodbye = standing(&mut connection, fireflies.id)
        .await
        .into_iter()
        .rfind(|word| word.text == "Friday.")
        .expect("Fireflies' goodbye");
    meeting_word::edit(
        &mut connection,
        fireflies.id,
        Replacement {
            range: Range {
                start_word_id: goodbye.id,
                end_word_id: goodbye.id,
            },
            text: "Friday!",
        },
        person(),
    )
    .await
    .expect("edits");
    // And a second, the name Fireflies misheard where Teams heard it too: two edits carried across
    // one merge stay two, each where it was said.
    let misheard = standing(&mut connection, fireflies.id)
        .await
        .into_iter()
        .find(|word| word.text == "Sheriff")
        .expect("the misheard name");
    meeting_word::edit(
        &mut connection,
        fireflies.id,
        Replacement {
            range: Range {
                start_word_id: misheard.id,
                end_word_id: misheard.id,
            },
            text: "Sharath",
        },
        person(),
    )
    .await
    .expect("edits");
    let pairs = pair_of(&mut connection, fireflies.id).await;
    let [pair] = pairs.as_slice() else {
        panic!("one pair, not {pairs:?}");
    };
    assert_eq!((pair.meeting_id, pair.same_as_id), (fireflies.id, teams.id));
    assert_eq!(pair.decision, None);
    let item_id = pair.action_item_id.expect("an inbox item asks");
    let item = action_item::find(&mut connection, item_id)
        .await
        .expect("the item");
    assert_eq!(item.title, "Same meeting as Launch review");
    assert_eq!(item.state, ActionItemState::Inbox);

    // Aligning again, as a retried attempt does, asks nothing more; aligning Teams again finds the
    // pair recorded, either way round.
    let again = align(&database, &mut connection, &fireflies).await;
    assert!(matches!(again, Done::Held { .. }));
    let teams_again = align(&database, &mut connection, &teams).await;
    assert!(matches!(teams_again, Done::Succeeded { .. }));
    assert_eq!(pair_of(&mut connection, teams.id).await.len(), 1);

    // Someone else cannot answer it.
    assert!(matches!(
        duplicates::merge(&mut connection, pair.id, Uuid::now_v7(), Utc::now()).await,
        Err(duplicates::DecideError::NotFound)
    ));

    // Ada merges it.
    let merged = duplicates::merge(&mut connection, pair.id, TEST_PERSON_ID, Utc::now())
        .await
        .expect("merges");
    assert_eq!(merged.pair.decision.as_deref(), Some("merged"));
    let fireflies_now = meeting::find(&mut connection, fireflies.id)
        .await
        .expect("found");
    assert_eq!(fireflies_now.merged_into, Some(teams.id));
    let captures = meeting_capture::list_of(&mut connection, teams.id)
        .await
        .expect("reads");
    assert_eq!(
        captures
            .iter()
            .map(|capture| capture.id)
            .collect::<Vec<_>>(),
        vec![teams_capture.id, fireflies_capture.id],
        "the capture Teams was listed with stays first"
    );
    // Placed from the listings' start times until aligning finds better.
    assert_eq!(
        (captures[1].offset_ms, captures[1].offset_source()),
        (0, OffsetSource::Listed)
    );
    let item = action_item::find(&mut connection, item_id)
        .await
        .expect("the item");
    assert_eq!(item.state, ActionItemState::Resolved);
    // Fireflies' own run goes on, and its steps skip the meeting merged away.
    let steps = workflow_run::steps_of(&mut connection, fireflies_run)
        .await
        .expect("steps");
    assert_eq!(steps[0].state, StepJobState::Succeeded);
    let correct_input: TranscribedMeeting =
        serde_json::from_value(steps[1].input.clone().expect("handed on")).expect("reads");
    let corrected = CorrectTranscript::for_test(database.clone())
        .run(
            correct_input,
            StepContext::for_test::<CorrectTranscript>(database.clone()),
        )
        .await
        .expect("an attempt");
    assert_eq!(
        corrected,
        Done::Skipped {
            reason: "Merged into another meeting, so nothing to correct".to_owned(),
        }
    );
    // Its run ends as its steps skipping the merged meeting end it.
    for step in &steps[1..] {
        let actor = workflow_run::Actor {
            user_id: crate::models::user::SYSTEM_USER_ID,
            at: Utc::now(),
        };
        step_job::skip(&mut connection, fireflies_run, step.id, actor)
            .await
            .expect("skips");
    }
    let finished = workflow_run::find(&mut connection, fireflies_run)
        .await
        .expect("the run");
    assert_eq!(finished.state, WorkflowRunState::Succeeded);
    // And a run aligning Teams again was started.
    let alignment = workflow_run::find_live(
        &mut connection,
        workflow::alignment().name(),
        workflow::SOURCE_KIND,
        &teams.id.to_string(),
    )
    .await
    .expect("reads")
    .expect("an alignment run");
    assert_eq!(alignment.state, WorkflowRunState::Queued);

    // That run finds Fireflies' place on the clock by the words the two share.
    let realigned = align(&database, &mut connection, &teams).await;
    assert!(matches!(realigned, Done::Succeeded { .. }), "{realigned:?}");
    let placed = meeting_capture::find(&mut connection, fireflies_capture.id)
        .await
        .expect("found");
    assert_eq!(placed.offset_source(), OffsetSource::Words);
    assert!(
        (placed.offset_ms - FIREFLIES_LATE_MS).abs() <= 300,
        "placed at {} ms",
        placed.offset_ms
    );
    // One transcript: Teams' words where Teams heard them, Ada's edit standing, the excerpt still
    // over the words it was made on, and the goodbyes only Fireflies heard, at forty seconds.
    let lined_up = standing(&mut connection, teams.id).await;
    let read = text(&lined_up);
    assert!(
        read.contains("Sharath confirmed the Elysium migration finishes Thursday night."),
        "{read}"
    );
    assert!(
        !read.contains("Sheriff"),
        "a moment Teams heard reads once: {read}"
    );
    assert_eq!(
        read.matches("Sharath").count(),
        1,
        "the name corrected on Fireflies stands once, in its place: {read}"
    );
    assert!(
        read.ends_with(
            "Mooreslab wants the customer interviews scheduled next week. Demo on Friday. Thanks \
             all, see you Friday!"
        ),
        "the edit carried from Fireflies stands where it was said, and what it replaced stays \
         out, though aligning moved its capture: {read}"
    );
    let carried = lined_up
        .iter()
        .find(|word| word.text == "Friday!")
        .expect("the carried edit");
    let you = lined_up
        .iter()
        .find(|word| word.text == "you" && word.capture_id == Some(fireflies_capture.id))
        .expect("the word before it");
    assert!(
        carried.start_ms >= you.end_ms && carried.start_ms - you.end_ms < 1_000,
        "the edit moved with its capture: {} after {}",
        carried.start_ms,
        you.end_ms
    );
    let thanks = lined_up
        .iter()
        .find(|word| word.text == "Thanks")
        .expect("Fireflies' goodbye");
    assert_eq!(thanks.capture_id, Some(fireflies_capture.id));
    assert!(
        (thanks.start_ms - 40_000).abs() <= 300,
        "{}",
        thanks.start_ms
    );
    assert_eq!(thanks.speaker, "Ada Lovelace");
    let excerpt_now = meeting_mark::find(&mut connection, teams.id, excerpt.id)
        .await
        .expect("the mark");
    assert_eq!(
        (excerpt_now.start_word_id, excerpt_now.end_word_id),
        (mooreslab.id, week.id),
        "words that did not change keep their ids, and the marks over them"
    );

    // Split it: the merge in Teams' history undone.
    let history = meeting_event::history(&mut connection, teams.id)
        .await
        .expect("history");
    let merge_entry = history
        .iter()
        .rev()
        .find(|entry| entry.kind() == Some(EventKind::Merged))
        .expect("the merge is in Teams' history")
        .clone();
    assert!(
        meeting_event::history(&mut connection, fireflies.id)
            .await
            .expect("history")
            .iter()
            .any(|entry| entry.kind() == Some(EventKind::Merged)),
        "and in Fireflies'"
    );
    // A person sets the moved capture's offset by hand: splitting now would overwrite that, so it
    // is refused until the hand-set offset is undone.
    let hand_set = clock::nudge(
        &mut connection,
        teams.id,
        fireflies_capture.id,
        FIREFLIES_LATE_MS + 100,
        person(),
    )
    .await
    .expect("nudges");
    assert!(matches!(
        history::undo(&mut connection, teams.id, merge_entry.id, person()).await,
        Err(history::UndoError::ChangedSince)
    ));
    history::undo(&mut connection, teams.id, hand_set.id, person())
        .await
        .expect("undoes the hand-set offset");
    let split = history::undo(&mut connection, teams.id, merge_entry.id, person())
        .await
        .expect("splits");
    assert_eq!(split.kind(), Some(EventKind::Split));
    let fireflies_now = meeting::find(&mut connection, fireflies.id)
        .await
        .expect("found");
    assert_eq!(fireflies_now.merged_into, None);
    let back = meeting_capture::find(&mut connection, fireflies_capture.id)
        .await
        .expect("found");
    assert_eq!(back.meeting_id, fireflies.id);
    let teams_alone = text(&standing(&mut connection, teams.id).await);
    assert!(
        teams_alone.ends_with("scheduled next week."),
        "{teams_alone}"
    );
    assert!(
        teams_alone.contains("Thursday night."),
        "the edit stands through it all"
    );
    let fireflies_alone = text(&standing(&mut connection, fireflies.id).await);
    assert!(
        fireflies_alone.contains("Sharath confirmed") && fireflies_alone.ends_with("Friday!"),
        "Fireflies reads as it did before the merge, Ada's edits and all: {fireflies_alone}"
    );
    let pairs = pair_of(&mut connection, fireflies.id).await;
    assert_eq!(pairs[0].decision.as_deref(), Some("not_the_same"));
    // Fireflies is processed again from aligning, since its run went through while merged.
    let carried_on = workflow_run::find_live(
        &mut connection,
        workflow::workflow().name(),
        workflow::SOURCE_KIND,
        &fireflies.id.to_string(),
    )
    .await
    .expect("reads")
    .expect("a run of its own again");
    assert_ne!(carried_on.id, fireflies_run);
    let steps = workflow_run::steps_of(&mut connection, carried_on.id)
        .await
        .expect("steps");
    assert_eq!(steps[0].step, AlignMeeting::NAME);
    // Aligned again, it no longer asks: the split said they are not the same.
    let alone = align(&database, &mut connection, &fireflies_now).await;
    assert!(matches!(alone, Done::Succeeded { .. }), "{alone:?}");

    // Undoing the split merges them again.
    let merged_again = history::undo(&mut connection, teams.id, split.id, person())
        .await
        .expect("merges again");
    assert_eq!(merged_again.kind(), Some(EventKind::Merged));
    let fireflies_now = meeting::find(&mut connection, fireflies.id)
        .await
        .expect("found");
    assert_eq!(fireflies_now.merged_into, Some(teams.id));
    assert_eq!(
        pair_of(&mut connection, fireflies.id).await[0]
            .decision
            .as_deref(),
        Some("merged")
    );
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn not_the_same_is_remembered_and_lets_the_run_go_on() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let on_the_hour: DateTime<Utc> = "2026-11-10T15:00:00Z".parse().expect("a time");
    let (first, _) =
        listed_and_fetched(&mut connection, "teams", on_the_hour, &teams_lines(), None).await;
    let (second, _) = listed_and_fetched(
        &mut connection,
        "fireflies",
        on_the_hour,
        &fireflies_lines(),
        None,
    )
    .await;
    assert!(matches!(
        align(&database, &mut connection, &first).await,
        Done::Succeeded { .. }
    ));
    let (run_id, held) = align_through_the_queue(&database, &mut connection, &second).await;
    assert!(matches!(held, Done::Held { .. }));
    let pair = pair_of(&mut connection, second.id).await.remove(0);

    let decided = duplicates::not_the_same(&mut connection, pair.id, TEST_PERSON_ID, Utc::now())
        .await
        .expect("decides");
    assert_eq!(decided.pair.decision.as_deref(), Some("not_the_same"));
    assert_eq!(decided.runs.len(), 1, "its waiting run is let through");
    let steps = workflow_run::steps_of(&mut connection, run_id)
        .await
        .expect("steps");
    assert_eq!(steps[0].state, StepJobState::Succeeded);
    assert_eq!(steps[1].state, StepJobState::Queued);
    // Decided once: it is never asked about again, and a second answer is refused.
    assert!(matches!(
        align(&database, &mut connection, &second).await,
        Done::Succeeded { .. }
    ));
    assert_eq!(pair_of(&mut connection, second.id).await.len(), 1);
    assert!(matches!(
        duplicates::merge(&mut connection, pair.id, TEST_PERSON_ID, Utc::now()).await,
        Err(duplicates::DecideError::Decided)
    ));
}

/// Merge and Not the same pressed at once, from two tabs: one answer counts, and what the pair
/// says matches what happened to the meeting.
#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
async fn two_answers_at_once_leave_the_pair_saying_what_happened() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let on_the_hour: DateTime<Utc> = "2026-11-10T15:00:00Z".parse().expect("a time");
    let (first, _) =
        listed_and_fetched(&mut connection, "teams", on_the_hour, &teams_lines(), None).await;
    let (second, _) = listed_and_fetched(
        &mut connection,
        "fireflies",
        on_the_hour,
        &fireflies_lines(),
        None,
    )
    .await;
    assert!(matches!(
        align(&database, &mut connection, &first).await,
        Done::Succeeded { .. }
    ));
    let (_run, held) = align_through_the_queue(&database, &mut connection, &second).await;
    assert!(matches!(held, Done::Held { .. }));
    let pair = pair_of(&mut connection, second.id).await.remove(0);

    let mut merging = database.get().await.expect("a connection");
    let mut keeping_apart = database.get().await.expect("a connection");
    let (merged, kept_apart) = tokio::join!(
        duplicates::merge(&mut merging, pair.id, TEST_PERSON_ID, Utc::now()),
        duplicates::not_the_same(&mut keeping_apart, pair.id, TEST_PERSON_ID, Utc::now()),
    );
    assert!(
        merged.is_ok() != kept_apart.is_ok(),
        "exactly one answer counts: {merged:?} {kept_apart:?}"
    );
    let refused = if merged.is_ok() { kept_apart } else { merged };
    assert!(matches!(refused, Err(duplicates::DecideError::Decided)));

    let decision = pair_of(&mut connection, second.id).await[0]
        .decision
        .clone();
    let now = meeting::find(&mut connection, second.id)
        .await
        .expect("found");
    match decision.as_deref() {
        Some("merged") => assert_eq!(now.merged_into, Some(first.id)),
        Some("not_the_same") => assert_eq!(now.merged_into, None),
        other => panic!("the pair is decided, not {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "an edit, a nudge, and both undone, with what each leaves of the words"
)]
async fn an_offset_set_by_hand_moves_its_captures_words_until_undone() {
    let (url, mut connection) = migrated_database().await;
    let database = crate::connections::connect_postgres(&url, 4)
        .await
        .expect("a pool");
    let on_the_hour: DateTime<Utc> = "2026-11-10T15:00:00Z".parse().expect("a time");
    let (meeting, capture) = listed_and_fetched(
        &mut connection,
        "fireflies",
        on_the_hour,
        &fireflies_lines(),
        None,
    )
    .await;
    assert!(matches!(
        align(&database, &mut connection, &meeting).await,
        Done::Succeeded { .. }
    ));
    let first_word = |words: &[Word]| words[0].start_ms;
    // An edit over the capture's words moves with them too, and what it replaced stays out.
    let heard = standing(&mut connection, meeting.id).await;
    let lab = heard
        .iter()
        .find(|word| word.text == "lab")
        .expect("a word")
        .clone();
    meeting_word::edit(
        &mut connection,
        meeting.id,
        Replacement {
            range: Range {
                start_word_id: lab.id,
                end_word_id: lab.id,
            },
            text: "Lab",
        },
        person(),
    )
    .await
    .expect("edits");
    let words_before = standing(&mut connection, meeting.id).await;
    let before = first_word(&words_before);
    let text_before = text(&words_before);

    let nudged = clock::nudge(
        &mut connection,
        meeting.id,
        capture.id,
        FIREFLIES_LATE_MS,
        person(),
    )
    .await
    .expect("nudges");
    assert_eq!(nudged.kind(), Some(EventKind::OffsetChanged));
    // Every word moves with its capture and keeps its id, so marks over them stay where they are.
    let words_after = standing(&mut connection, meeting.id).await;
    assert_eq!(
        text(&words_after),
        text_before,
        "the edit stands, and lab stays out"
    );
    assert_eq!(
        words_after.iter().map(|word| word.id).collect::<Vec<_>>(),
        words_before.iter().map(|word| word.id).collect::<Vec<_>>()
    );
    assert!(
        words_after
            .iter()
            .zip(&words_before)
            .all(|(after, before)| after.start_ms == before.start_ms + FIREFLIES_LATE_MS)
    );
    let moved = meeting_capture::find(&mut connection, capture.id)
        .await
        .expect("found");
    assert_eq!(
        (moved.offset_ms, moved.offset_source()),
        (FIREFLIES_LATE_MS, OffsetSource::Hand)
    );
    assert_eq!(
        first_word(&standing(&mut connection, meeting.id).await),
        before + FIREFLIES_LATE_MS
    );
    // Aligning never moves an offset a person set.
    assert!(matches!(
        align(&database, &mut connection, &meeting).await,
        Done::Succeeded { .. }
    ));
    assert_eq!(
        meeting_capture::find(&mut connection, capture.id)
            .await
            .expect("found")
            .offset_ms,
        FIREFLIES_LATE_MS
    );
    assert!(matches!(
        clock::nudge(
            &mut connection,
            meeting.id,
            capture.id,
            clock::OFFSET_MAX_MS + 1,
            person()
        )
        .await,
        Err(clock::NudgeError::OutOfRange)
    ));

    history::undo(&mut connection, meeting.id, nudged.id, person())
        .await
        .expect("undoes");
    let back = meeting_capture::find(&mut connection, capture.id)
        .await
        .expect("found");
    assert_eq!(
        (back.offset_ms, back.offset_source()),
        (0, OffsetSource::Listed)
    );
    let words_back = standing(&mut connection, meeting.id).await;
    assert_eq!(first_word(&words_back), before);
    assert_eq!(text(&words_back), text_before);
    // Undoing the edit now stands what it replaced up again, once, at the capture's times.
    let edit_entry = meeting_event::history(&mut connection, meeting.id)
        .await
        .expect("history")
        .into_iter()
        .find(|entry| entry.kind() == Some(EventKind::Edited))
        .expect("the edit");
    history::undo(&mut connection, meeting.id, edit_entry.id, person())
        .await
        .expect("undoes the edit");
    let unedited = text(&standing(&mut connection, meeting.id).await);
    assert_eq!(unedited.matches("Moore's lab").count(), 1, "{unedited}");
    // Undone once; the undo of a change moved on since is refused.
    assert!(matches!(
        history::undo(&mut connection, meeting.id, nudged.id, person()).await,
        Err(history::UndoError::ChangedSince)
    ));
}
