// Copyright © 2026 Jalapeno Labs

//! A meeting's final transcript: a stream of words, each with a stable id. See
//! `docs/meetings.md`, The final transcript.
//!
//! Every word has a speaker, a start and end on the meeting's clock, and where it came from: the
//! source transcript of the capture it was heard in, that capture's Whisper transcript, a
//! correction of what the capture heard, or an edit. Marks (`models::meeting_mark`) are anchored
//! to a range of word ids, so a mark is one word or the whole meeting.
//!
//! # Every word it ever held
//!
//! A word is never deleted, and what it says is never rewritten: its text, its speaker, and where
//! it came from. Only its times move, with its capture's place on the meeting's clock (Building it
//! again, below). An edit ([`edit`]) marks the words of its range as
//! replaced (`superseded_by`) and writes new ones in their place, so an undo ([`undo`]) puts the
//! old words back and every mark and history entry naming a word still resolves. The transcript is
//! the words that stand, in order of `position`.
//!
//! `position` orders every word the meeting ever held, standing or replaced, in one sequence with
//! gaps of [`POSITION_GAP`]. An edit writes its new words just after the last word it replaces and
//! before whatever came next, so the words it replaced stay exactly where they were, and an undo
//! that stands them up again finds them between the same neighbours. When no gap is left there,
//! the meeting's words are renumbered in one statement, keeping their order.
//!
//! # Marks over an edit
//!
//! Marks over the words an edit replaces cover the words that replace them: a mark starting in
//! the range starts at the first new word, and one ending in it ends at the last. The edit also
//! makes a mark of its own, of kind `edit`, over its new words. Its history entry records every
//! mark it moved, so its undo moves each back exactly where it was, unless a later change moved
//! it since; marks made over the new words afterwards move onto the words put back the same way.
//!
//! # Building it again
//!
//! A transcript is lined up from its captures ([`rebuild`]) when they are first aligned, and again
//! whenever what it is lined up from changes: a capture's offset, a capture merged in or split
//! back out, a corrected copy. A rebuild compares each capture's words as it gives them now with
//! its words that stand, in order, by what each says (its text, its speaker, and where it came
//! from) and not by when: a word that says the same keeps its id, so marks over it never move, and
//! takes its new times, so a capture placed anew on the clock moves its words rather than
//! replacing them. The rest are replaced by the rebuild.
//! Every edit's words stand through it, and so do the words the meeting's correction settled while
//! their capture is the meeting's, so a rebuild never undoes what anyone changed, and the captures'
//! words inside an edit's time stay out; a settlement is an edit here ([`Overwriting`]), and what
//! follows of edits holds for it. An edit belongs to the capture of the words it replaced, and it and they move with that capture: by as much as its offset moved since its words
//! were last placed (the newest `built` entry naming it, or the merge that brought it), so an edit
//! stays over what it replaced wherever its capture goes on the clock. A standing word of another
//! capture under an edit is replaced too, even one that says the same, but only once the edit's
//! capture is placed on the clock: one still at its listed offset sits at a time nobody has
//! checked. A mark over words a rebuild replaced moves onto the new words at the same moment. A
//! rebuild is not undone on its own: what caused it is (a hand-set offset, a merge), and that
//! builds the transcript again.
//!
//! # Settling it
//!
//! A meeting's correction settles its transcript once ([`settle`]): over each range it flagged it
//! writes the text it decided on, as words of its own (origin `correction`), and a confidence mark
//! with its confidence and alternatives, and it marks each range a capture heard otherwise. It is
//! one change in the meeting's history, and a rebuild keeps what it wrote, so a hand-set offset, a
//! merge, or a split builds the transcript again without settling it again. A merge carries only a
//! person's edits into the meeting the captures go into, and a settled word of a capture split back
//! out goes with its capture.
//!
//! Every change is made under the meeting's lock and recorded in its history
//! (`models::meeting_event`) in the same transaction.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use similar::{Algorithm, DiffOp};
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::schema::{meeting_marks, meeting_words};
use crate::models::agent::Author;
use crate::models::meeting;
use crate::models::meeting_event::{self, Change, EventKind, MeetingEvent};
use crate::models::meeting_mark::{self, Mark, MarkKind, NewMark, Range};

/// The gap between two neighbouring words' positions as a transcript is built or renumbered:
/// room for an edit to write a thousand words between any two before the meeting's words need
/// renumbering. `crate::meetings::final_transcript` and the migration that split meetings into
/// captures number with it too.
pub const POSITION_GAP: i64 = 1024;

/// The most characters one edit writes: a long paragraph rewritten by hand, and far less than a
/// request body allows.
pub const EDIT_TEXT_MAX_CHARACTERS: usize = 20_000;

/// How many words one insert writes. Each word binds ten parameters, and Postgres takes at most
/// 65,535 in one statement.
const INSERT_CHUNK: usize = 1_000;

/// Where a word came from. Stored as `meeting_words.origin`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// Its capture's source transcript, as the source sent it.
    Source,
    /// Its capture's Whisper transcript.
    Whisper,
    /// A correction of what its capture heard.
    Correction,
    /// An edit, by a person or an agent; no capture said it.
    Edit,
}

impl Origin {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Whisper => "whisper",
            Self::Correction => "correction",
            Self::Edit => "edit",
        }
    }
}

/// What the first capture's words of a meeting's final transcript were lined up from. Stored in
/// the `built` entry of its history, where an entry that says `correction` lined them up from the
/// meeting's corrected copy's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltFrom {
    /// Its capture's source transcript.
    Source,
    /// Its capture's Whisper transcript, for a source that sent no lines.
    Whisper,
}

/// One word of a meeting's final transcript, standing or replaced.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = meeting_words, check_for_backend(diesel::pg::Pg))]
pub struct Word {
    pub id: Uuid,
    pub meeting_id: Uuid,
    pub position: i64,
    pub text: String,
    pub speaker: String,
    /// On the meeting's clock.
    pub start_ms: i64,
    pub end_ms: i64,
    /// An [`Origin`] name.
    pub origin: String,
    /// The capture it was heard in; `None` for an edit's words.
    pub capture_id: Option<Uuid>,
    /// The change that wrote it.
    pub introduced_by: Uuid,
    /// The change that replaced it; `None` while it stands.
    pub superseded_by: Option<Uuid>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = meeting_words)]
struct NewWord {
    id: Uuid,
    meeting_id: Uuid,
    position: i64,
    text: String,
    speaker: String,
    start_ms: i64,
    end_ms: i64,
    origin: String,
    capture_id: Option<Uuid>,
    introduced_by: Uuid,
}

/// One word of a transcript about to be built, already on the meeting's clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltWord {
    pub text: String,
    pub speaker: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub origin: Origin,
}

/// One word a capture heard, as a transcript is lined up from its captures.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeardWord {
    pub capture_id: Uuid,
    pub word: BuiltWord,
}

/// What a meeting's final transcript is lined up from: every capture's words, in order, and what
/// its history entry says of them.
#[derive(Debug, Clone)]
pub struct Rebuild {
    pub meeting_id: Uuid,
    /// In order, on the meeting's clock.
    pub words: Vec<HeardWord>,
    /// Each capture it was lined up from, in the meeting's order, with its offset.
    pub captures: Vec<BuiltCapture>,
    /// What the first capture's words are lined up from.
    pub built_from: BuiltFrom,
    /// The captures after the first still at the offset their listing gave, not yet aligned: an
    /// edit over one of them sits at a time nobody has checked, so it keeps out no other capture's
    /// standing word until its capture is placed.
    pub provisional: HashSet<Uuid>,
}

/// One capture a transcript was lined up from, as its history entry records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuiltCapture {
    pub capture_id: Uuid,
    pub offset_ms: i64,
}

/// What a rebuild records in the meeting's history, as its `built` entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rebuilt {
    pub captures: Vec<BuiltCapture>,
    pub built_from: BuiltFrom,
    /// How many words stand after it.
    pub words: usize,
    /// How many words it wrote, how many it replaced, and how many it kept at new times.
    pub added: usize,
    pub replaced: usize,
    pub retimed: usize,
    /// How many marks it moved off the words it replaced.
    pub marks_moved: usize,
}

/// A word that stands after a rebuild: one standing already, kept, or one the rebuild writes.
#[derive(Debug, Clone)]
enum After {
    Kept(Word),
    Written(NewWord),
}

impl After {
    const fn id(&self) -> Uuid {
        match self {
            Self::Kept(word) => word.id,
            Self::Written(word) => word.id,
        }
    }

    const fn span(&self) -> (i64, i64) {
        match self {
            Self::Kept(word) => (word.start_ms, word.end_ms),
            Self::Written(word) => (word.start_ms, word.end_ms),
        }
    }
}

/// Lines meeting `lined_up.meeting_id`'s final transcript up from `lined_up` as `author`, under the
/// meeting's lock: the words that differ from what stands are replaced, every edit's words stand
/// through it, and marks over replaced words move onto the new words at the same moment (the
/// module's docs, Building it again). Answers the `built` history entry, or `None`, writing
/// nothing, when the transcript already reads exactly so.
///
/// # Errors
/// Propagates any database error, such as a meeting that does not exist.
pub async fn rebuild(
    connection: &mut AsyncPgConnection,
    lined_up: Rebuild,
    author: Author<'_>,
) -> QueryResult<Option<MeetingEvent>> {
    connection
        .transaction(async |connection| {
            let meeting_id = lined_up.meeting_id;
            meeting::lock(connection, meeting_id).await?;
            let every_word: Vec<Word> = meeting_words::table
                .filter(meeting_words::meeting_id.eq(meeting_id))
                .order(meeting_words::position.asc())
                .select(Word::as_select())
                .load(connection)
                .await?;

            let moved_by = moved_since_built(connection, meeting_id, &lined_up.captures).await?;
            let present: HashSet<Uuid> = lined_up
                .captures
                .iter()
                .map(|capture| capture.capture_id)
                .collect();
            let event_id = Uuid::now_v7();
            let Compared {
                built,
                replaced,
                times,
                edits,
            } = compare(
                &every_word,
                lined_up.words,
                (&moved_by, &lined_up.provisional),
                Overwriting::EditsAndSettled(&present),
                (meeting_id, event_id),
            );
            let added = built
                .iter()
                .filter(|word| matches!(word, After::Written(_)))
                .count();
            let retimed = every_word
                .iter()
                .filter(|word| {
                    times
                        .get(&word.id)
                        .is_some_and(|span| *span != (word.start_ms, word.end_ms))
                })
                .count();
            if added == 0 && replaced.is_empty() && retimed == 0 {
                return Ok(None);
            }
            let standing = with_edits(built, &edits);
            let ordered = in_order(&every_word, &standing);
            let anchored = meeting_mark::anchored_in(connection, meeting_id, &replaced).await?;
            let moves = moves_by_time(&anchored, &every_word, &standing);

            let recorded = Rebuilt {
                captures: lined_up.captures,
                built_from: lined_up.built_from,
                words: standing.len(),
                added,
                replaced: replaced.len(),
                retimed,
                marks_moved: moves.len(),
            };
            let entry = meeting_event::record(
                connection,
                Change {
                    id: event_id,
                    meeting_id,
                    kind: EventKind::Built,
                    author,
                    data: serde_json::to_value(&recorded).expect("a rebuild is plain fields"),
                    undid: None,
                },
            )
            .await?;
            supersede(connection, &replaced, event_id).await?;
            write_order(connection, &every_word, standing, &ordered, &times).await?;
            move_marks(connection, &moves).await?;
            Ok(Some(entry))
        })
        .await
}

/// Which words were written over what a capture heard and stand through every rebuild, as edits:
/// a person's edits always, and the words the meeting's correction settled while their capture is
/// still one of the meeting's.
#[derive(Debug, Clone, Copy)]
enum Overwriting<'present> {
    /// A person's edits alone, as a merge carries them into another meeting.
    Edits,
    /// Edits, and settled words of the captures `present`, as a rebuild keeps them.
    EditsAndSettled(&'present HashSet<Uuid>),
}

impl Overwriting<'_> {
    /// Whether `word`, standing or not, was written over what a capture heard.
    fn wrote(self, word: &Word) -> bool {
        if word.origin == Origin::Edit.as_str() {
            return true;
        }
        let Self::EditsAndSettled(present) = self else {
            return false;
        };
        word.origin == Origin::Correction.as_str()
            && word
                .capture_id
                .is_some_and(|capture_id| present.contains(&capture_id))
    }
}

/// The changes whose words stand in place of words a capture heard: every edit whose words
/// stand, and every edit (or carried edit) whose own words an edit that stands replaced in turn,
/// so the words at the bottom of a chain of edits are still known to be edited over. What counts
/// as an edit is `overwriting`'s.
fn covering_edits(every_word: &[Word], overwriting: Overwriting<'_>) -> HashSet<Uuid> {
    let is_edit = |word: &Word| overwriting.wrote(word);
    let mut covering: HashSet<Uuid> = every_word
        .iter()
        .filter(|word| is_edit(word) && word.superseded_by.is_none())
        .map(|word| word.introduced_by)
        .collect();
    loop {
        let deeper: Vec<Uuid> = every_word
            .iter()
            .filter(|word| {
                is_edit(word)
                    && word
                        .superseded_by
                        .is_some_and(|replacing| covering.contains(&replacing))
                    && !covering.contains(&word.introduced_by)
            })
            .map(|word| word.introduced_by)
            .collect();
        if deeper.is_empty() {
            return covering;
        }
        covering.extend(deeper);
    }
}

/// Whether `word` is one an edit standing now replaced (`covering`, from [`covering_edits`]): a word
/// a capture heard, or one a correction settled, which names the capture an edit over it belongs
/// to. A person's own edit replaced by another is no such word: it names no capture.
fn is_covered(word: &Word, covering: &HashSet<Uuid>) -> bool {
    word.origin != Origin::Edit.as_str()
        && word
            .superseded_by
            .is_some_and(|replacing| covering.contains(&replacing))
}

/// The standing edits of `every_word` (in order of position): each run of standing edit words with
/// no other standing word between, and no word an edit replaced either, since each edit's replaced
/// words sit just before its own (two edits of neighbouring words, or two edits a merge carried in
/// one after the other, are two runs). Ordered by when each begins, so an edit carried in from
/// another meeting goes where it was said rather than where its words were written. What counts as
/// an edit is `overwriting`'s.
fn edit_runs(every_word: &[Word], overwriting: Overwriting<'_>) -> Vec<Vec<Word>> {
    let covering = covering_edits(every_word, overwriting);
    let mut runs: Vec<Vec<Word>> = Vec::new();
    let mut in_run = false;
    for word in every_word {
        if is_covered(word, &covering) {
            in_run = false;
            continue;
        }
        if word.superseded_by.is_some() {
            continue;
        }
        if !overwriting.wrote(word) {
            in_run = false;
            continue;
        }
        match runs.last_mut() {
            Some(run) if in_run => run.push(word.clone()),
            _ => runs.push(vec![word.clone()]),
        }
        in_run = true;
    }
    runs.sort_by_key(|run| run[0].start_ms);
    runs
}

/// Whether the middle of `word` lies inside the time one of the edits `runs` covers: what an edit
/// replaced, which no capture's word comes back over.
fn inside_any(runs: &[Vec<Word>], word: &BuiltWord) -> bool {
    let middle = word.start_ms + (word.end_ms - word.start_ms) / 2;
    runs.iter().any(|run| {
        let start = run[0].start_ms;
        let end = run[run.len() - 1].end_ms.max(start);
        (start..=end).contains(&middle)
    })
}

/// What a word says, as two builds are compared: its text, its speaker, and where it came from.
/// Its times are not part of it, so a capture placed anew keeps its words.
type WordKey<'word> = (&'word str, &'word str, &'word str);

/// How long one capture's comparison may take before it settles for a diff that is correct but
/// not the smallest. Words that say the same compare in linear time whatever their times, so only
/// a capture whose words all changed comes near it, and it is then replaced nearly whole anyway.
const DIFF_DEADLINE: std::time::Duration = std::time::Duration::from_secs(2);

/// What comparing a rebuild's words with the meeting's found.
struct Compared {
    /// Every capture word of the rebuild that stands, in its order: kept or written anew.
    built: Vec<After>,
    /// The standing words no longer heard, which the rebuild replaces.
    replaced: Vec<Uuid>,
    /// The times of every word already the meeting's that the rebuild moves on the clock: the
    /// captures' words it kept, the words edits replaced, and the edits'
    /// own words, which move with them.
    times: HashMap<Uuid, (i64, i64)>,
    /// Every standing edit's run of words at its new times, ordered by when it begins.
    edits: Vec<Vec<Word>>,
}

/// The words `heard` gives against the meeting's words `every_word` (in order of position),
/// capture by capture, by what each says (`matched`):
///
/// - a heard word matching a standing word keeps it, at the heard word's times;
/// - any other heard word is written anew by change `event_id`;
/// - a standing word no heard word matches is replaced;
/// - but a heard word inside an edit's time stays out, and a standing word it matched is
///   replaced: what an edit replaced, or another capture's word a move brought under the edit.
///
/// The words edits replaced, and the edits' own words, move with their capture: by as much as its
/// offset moved since the transcript was last lined up (`moved_by`, by capture), so an edit stays
/// over what it replaced wherever its capture goes on the clock. An edit's words sit just after
/// the words they replaced, so an edit belongs to the capture of the nearest replaced word before
/// it. What counts as an edit is `overwriting`'s.
fn compare(
    every_word: &[Word],
    heard: Vec<HeardWord>,
    (moved_by, provisional): (&HashMap<Uuid, i64>, &HashSet<Uuid>),
    overwriting: Overwriting<'_>,
    (meeting_id, event_id): (Uuid, Uuid),
) -> Compared {
    let comparable: Vec<&Word> = every_word
        .iter()
        .filter(|word| word.superseded_by.is_none() && !overwriting.wrote(word))
        .collect();
    let (kept, still_heard) = matched(&comparable, &heard);

    let mut times: HashMap<Uuid, (i64, i64)> = HashMap::new();
    for (heard_word, kept_word) in heard.iter().zip(&kept) {
        if let Some(word) = kept_word {
            times.insert(word.id, (heard_word.word.start_ms, heard_word.word.end_ms));
        }
    }
    let (moved, capture_of_edit) = with_edits_moved(every_word, moved_by, overwriting, &mut times);
    let edits = edit_runs(&moved, overwriting);
    let placed: Vec<Vec<Word>> = edits
        .iter()
        .filter(|run| {
            capture_of_edit
                .get(&run[0].id)
                .is_none_or(|capture_id| !provisional.contains(capture_id))
        })
        .cloned()
        .collect();

    // A word inside an edit's time stays out, kept or new: what the edit replaced, or another
    // capture's word at the same moment, which a capture moved onto it brings there. A standing
    // word is replaced so only under an edit whose capture is placed on the clock.
    let mut edited_over: HashSet<Uuid> = HashSet::new();
    let built = heard
        .into_iter()
        .zip(kept)
        .filter_map(|(heard, kept)| match kept {
            Some(word) if inside_any(&placed, &heard.word) => {
                edited_over.insert(word.id);
                None
            }
            Some(word) => {
                let mut word = word.clone();
                (word.start_ms, word.end_ms) = (heard.word.start_ms, heard.word.end_ms);
                Some(After::Kept(word))
            }
            None if inside_any(&edits, &heard.word) => None,
            None => Some(After::Written(NewWord {
                id: Uuid::now_v7(),
                meeting_id,
                position: 0,
                text: heard.word.text,
                speaker: heard.word.speaker,
                start_ms: heard.word.start_ms,
                end_ms: heard.word.end_ms,
                origin: heard.word.origin.as_str().to_owned(),
                capture_id: Some(heard.capture_id),
                introduced_by: event_id,
            })),
        })
        .collect();
    let replaced = comparable
        .iter()
        .zip(still_heard)
        .filter(|(word, heard)| !heard || edited_over.contains(&word.id))
        .map(|(word, _heard)| word.id)
        .collect();
    Compared {
        built,
        replaced,
        times,
        edits,
    }
}

/// `every_word` with each word an edit replaced, and each standing edit word, moved by as much as
/// its capture moved (`moved_by`), each recorded in `times`, and the capture each standing edit
/// word belongs to: the capture of the nearest replaced word before it. What counts as an edit is
/// `overwriting`'s.
fn with_edits_moved(
    every_word: &[Word],
    moved_by: &HashMap<Uuid, i64>,
    overwriting: Overwriting<'_>,
    times: &mut HashMap<Uuid, (i64, i64)>,
) -> (Vec<Word>, HashMap<Uuid, Uuid>) {
    let covering = covering_edits(every_word, overwriting);
    let mut nearest_capture: Option<Uuid> = None;
    let mut capture_of_edit: HashMap<Uuid, Uuid> = HashMap::new();
    let mut moved = Vec::with_capacity(every_word.len());
    for word in every_word {
        let mut word = word.clone();
        let covered = is_covered(&word, &covering);
        if covered {
            nearest_capture = word.capture_id;
        }
        let stands_as_edit = word.superseded_by.is_none() && overwriting.wrote(&word);
        let capture = if covered {
            word.capture_id
        } else if stands_as_edit {
            if let Some(capture_id) = nearest_capture {
                capture_of_edit.insert(word.id, capture_id);
            }
            nearest_capture
        } else {
            None
        };
        let shift = capture
            .and_then(|capture_id| moved_by.get(&capture_id))
            .copied()
            .unwrap_or(0);
        if shift != 0 {
            word.start_ms += shift;
            word.end_ms += shift;
            times.insert(word.id, (word.start_ms, word.end_ms));
        }
        moved.push(word);
    }
    (moved, capture_of_edit)
}

/// How far each capture of `lined_up` moved on the clock since its words in meeting `meeting_id`
/// were last placed: its offset now less the one they were placed at. That is the offset the
/// meeting's newest `built` entry naming it recorded, or, for a capture merged in since, the one
/// the merge placed it at, which the edits it carried were written at. A capture neither names has
/// words nowhere in the meeting, and has not moved.
async fn moved_since_built(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    lined_up: &[BuiltCapture],
) -> QueryResult<HashMap<Uuid, i64>> {
    use crate::database::schema::meeting_events;

    let placing: Vec<(String, Value)> = meeting_events::table
        .filter(meeting_events::meeting_id.eq(meeting_id))
        .filter(
            meeting_events::kind.eq_any([EventKind::Built.as_str(), EventKind::Merged.as_str()]),
        )
        .order(meeting_events::id.desc())
        .select((meeting_events::kind, meeting_events::data))
        .load(connection)
        .await?;
    let mut placed_at: HashMap<Uuid, i64> = HashMap::new();
    for (kind, data) in placing {
        for (capture_id, offset_ms) in placed_by(&kind, meeting_id, data) {
            // The newest entry naming a capture is where its words sit now.
            placed_at.entry(capture_id).or_insert(offset_ms);
        }
        if lined_up
            .iter()
            .all(|capture| placed_at.contains_key(&capture.capture_id))
        {
            break;
        }
    }
    Ok(lined_up
        .iter()
        .filter_map(|capture| {
            let was = placed_at.get(&capture.capture_id)?;
            Some((capture.capture_id, capture.offset_ms - was))
        })
        .collect())
}

/// Each capture a `built` or `merged` entry of meeting `meeting_id` placed, and the offset it
/// placed it at.
fn placed_by(kind: &str, meeting_id: Uuid, data: Value) -> Vec<(Uuid, i64)> {
    if kind == EventKind::Merged.as_str() {
        return match serde_json::from_value::<MergedPlacements>(data) {
            // Both meetings record a merge; only the one the captures went into holds them.
            Ok(merged) if merged.into == meeting_id => merged
                .captures
                .into_iter()
                .map(|capture| (capture.capture_id, capture.after.offset_ms))
                .collect(),
            _ => Vec::new(),
        };
    }
    match serde_json::from_value::<BuiltOffsets>(data.clone()) {
        Ok(built) => built
            .captures
            .into_iter()
            .map(|capture| (capture.capture_id, capture.offset_ms))
            .collect(),
        // A transcript built before captures were aligned names its one capture, at the zero
        // offset every capture had then.
        Err(_old_shape) => data
            .get("captureId")
            .and_then(Value::as_str)
            .and_then(|capture_id| capture_id.parse().ok())
            .map(|capture_id| vec![(capture_id, 0)])
            .unwrap_or_default(),
    }
}

/// The captures a `built` entry lined its transcript up from, which is all a rebuild reads of it.
#[derive(Debug, Deserialize)]
struct BuiltOffsets {
    captures: Vec<BuiltCapture>,
}

/// Where a `merged` entry (`crate::meetings::duplicates::Moved`) placed each capture, which is all
/// a rebuild reads of it.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergedPlacements {
    into: Uuid,
    captures: Vec<MergedCapture>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergedCapture {
    capture_id: Uuid,
    after: MergedOffset,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MergedOffset {
    offset_ms: i64,
}

/// Matches the rebuild's words `heard` with the meeting's words `comparable`, each capture's on
/// its own, by what they say: for each word of `heard` the word it matches, if any, and for each
/// word of `comparable` whether it is heard again.
fn matched<'word>(
    comparable: &[&'word Word],
    heard: &[HeardWord],
) -> (Vec<Option<&'word Word>>, Vec<bool>) {
    let mut comparable_by_capture: HashMap<Option<Uuid>, Vec<usize>> = HashMap::new();
    for (index, word) in comparable.iter().enumerate() {
        comparable_by_capture
            .entry(word.capture_id)
            .or_default()
            .push(index);
    }
    let mut heard_by_capture: HashMap<Option<Uuid>, Vec<usize>> = HashMap::new();
    for (index, word) in heard.iter().enumerate() {
        heard_by_capture
            .entry(Some(word.capture_id))
            .or_default()
            .push(index);
    }

    // Each capture's words fill slots of their own, so the captures may be compared in any order.
    let mut kept: Vec<Option<&'word Word>> = vec![None; heard.len()];
    let mut still_heard = vec![false; comparable.len()];
    for (capture_id, heard_indices) in &heard_by_capture {
        let Some(comparable_indices) = comparable_by_capture.get(capture_id) else {
            continue;
        };
        let comparable_keys: Vec<WordKey> = comparable_indices
            .iter()
            .map(|index| {
                let word = comparable[*index];
                (
                    word.text.as_str(),
                    word.speaker.as_str(),
                    word.origin.as_str(),
                )
            })
            .collect();
        let heard_keys: Vec<WordKey> = heard_indices
            .iter()
            .map(|index| {
                let word = &heard[*index].word;
                (
                    word.text.as_str(),
                    word.speaker.as_str(),
                    word.origin.as_str(),
                )
            })
            .collect();
        let deadline = std::time::Instant::now() + DIFF_DEADLINE;
        let operations = similar::capture_diff_slices_deadline(
            Algorithm::Myers,
            &comparable_keys,
            &heard_keys,
            Some(deadline),
        );
        for operation in operations {
            if let DiffOp::Equal {
                old_index,
                new_index,
                len,
            } = operation
            {
                for step in 0..len {
                    let comparable_index = comparable_indices[old_index + step];
                    kept[heard_indices[new_index + step]] = Some(comparable[comparable_index]);
                    still_heard[comparable_index] = true;
                }
            }
        }
    }
    (kept, still_heard)
}

/// The transcript as it stands after a rebuild: the captures' words `built`, in order, with each
/// edit's run of words among them where it begins.
fn with_edits(built: Vec<After>, edited: &[Vec<Word>]) -> Vec<After> {
    let mut standing = Vec::with_capacity(built.len() + edited.iter().map(Vec::len).sum::<usize>());
    let mut runs = edited.iter().peekable();
    for word in built {
        while let Some(run) = runs.next_if(|run| run[0].start_ms <= word.span().0) {
            standing.extend(run.iter().cloned().map(After::Kept));
        }
        standing.push(word);
    }
    for run in runs {
        standing.extend(run.iter().cloned().map(After::Kept));
    }
    standing
}

/// Every word of the meeting in the order its positions give after a rebuild: the words that
/// stand, `standing`, in order, and each word that does not (replaced before, or by the rebuild)
/// just before the standing word that followed it in `every_word`, so a word an edit replaced still
/// sits just before the edit's words, where the edit's undo stands it up again.
fn in_order(every_word: &[Word], standing: &[After]) -> Vec<Uuid> {
    let stands: HashSet<Uuid> = standing.iter().map(After::id).collect();
    let mut before: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    let mut waiting: Vec<Uuid> = Vec::new();
    for word in every_word {
        if stands.contains(&word.id) {
            before.insert(word.id, std::mem::take(&mut waiting));
        } else {
            waiting.push(word.id);
        }
    }
    let mut ordered = Vec::with_capacity(every_word.len() + standing.len());
    for word in standing {
        if let Some(attached) = before.remove(&word.id()) {
            ordered.extend(attached);
        }
        ordered.push(word.id());
    }
    ordered.append(&mut waiting);
    ordered
}

/// Where each mark of `anchored` goes once the words of `standing` stand: an end on a word that no
/// longer stands moves, by time, to the first standing word that ends after the old word begins
/// (a start) or the last that begins before it ends (an end), and a range that would run backwards
/// becomes the one word it starts at. Marks left where they are are not listed.
fn moves_by_time(anchored: &[Mark], every_word: &[Word], standing: &[After]) -> Vec<MovedMark> {
    let place: HashMap<Uuid, usize> = standing
        .iter()
        .enumerate()
        .map(|(index, word)| (word.id(), index))
        .collect();
    let spans: HashMap<Uuid, (i64, i64)> = every_word
        .iter()
        .map(|word| (word.id, (word.start_ms, word.end_ms)))
        .collect();
    let (Some(first), Some(last)) = (standing.first(), standing.last()) else {
        // Nothing stands to move a mark onto; a mark keeps its words until something does.
        return Vec::new();
    };
    let starting = |word_id: Uuid| -> usize {
        if let Some(index) = place.get(&word_id) {
            return *index;
        }
        let begins = spans.get(&word_id).map_or(first.span().0, |span| span.0);
        standing
            .iter()
            .position(|word| word.span().1 >= begins)
            .unwrap_or(standing.len() - 1)
    };
    let ending = |word_id: Uuid| -> usize {
        if let Some(index) = place.get(&word_id) {
            return *index;
        }
        let ends = spans.get(&word_id).map_or(last.span().1, |span| span.1);
        standing
            .iter()
            .rposition(|word| word.span().0 <= ends)
            .unwrap_or(0)
    };
    let mut moves = Vec::new();
    for mark in anchored {
        let current = mark.range();
        let start = starting(current.start_word_id);
        let end = ending(current.end_word_id).max(start);
        let moved_to = Range {
            start_word_id: standing[start].id(),
            end_word_id: standing[end].id(),
        };
        if moved_to != current {
            moves.push(MovedMark {
                id: mark.id,
                before: current,
                after: moved_to,
            });
        }
    }
    moves
}

/// Writes a rebuild's order, `ordered`, [`POSITION_GAP`] apart, and its kept words' new times:
/// every word already the meeting's whose place or times change moves in one statement, which
/// `meeting_words_order`, deferrable, checks once at its end, and every word of `standing` the
/// rebuild writes is inserted at its place.
async fn write_order(
    connection: &mut AsyncPgConnection,
    every_word: &[Word],
    standing: Vec<After>,
    ordered: &[Uuid],
    times: &HashMap<Uuid, (i64, i64)>,
) -> QueryResult<()> {
    let positions: HashMap<Uuid, i64> = ordered
        .iter()
        .zip(1_i64..)
        .map(|(id, number)| (*id, number * POSITION_GAP))
        .collect();
    let mut moved_ids = Vec::new();
    let mut moved_positions = Vec::new();
    let mut moved_starts = Vec::new();
    let mut moved_ends = Vec::new();
    for word in every_word {
        let position = positions[&word.id];
        let (start_ms, end_ms) = times
            .get(&word.id)
            .copied()
            .unwrap_or((word.start_ms, word.end_ms));
        if (position, start_ms, end_ms) == (word.position, word.start_ms, word.end_ms) {
            continue;
        }
        moved_ids.push(word.id);
        moved_positions.push(position);
        moved_starts.push(start_ms);
        moved_ends.push(end_ms);
    }
    if !moved_ids.is_empty() {
        diesel::sql_query(
            "UPDATE meeting_words \
             SET position = moved.position, start_ms = moved.start_ms, end_ms = moved.end_ms \
             FROM unnest($1::uuid[], $2::bigint[], $3::bigint[], $4::bigint[]) \
                 AS moved (id, position, start_ms, end_ms) \
             WHERE meeting_words.id = moved.id",
        )
        .bind::<diesel::sql_types::Array<SqlUuid>, _>(&moved_ids)
        .bind::<diesel::sql_types::Array<BigInt>, _>(&moved_positions)
        .bind::<diesel::sql_types::Array<BigInt>, _>(&moved_starts)
        .bind::<diesel::sql_types::Array<BigInt>, _>(&moved_ends)
        .execute(connection)
        .await?;
    }
    let written: Vec<NewWord> = standing
        .into_iter()
        .filter_map(|word| match word {
            After::Written(mut new) => {
                new.position = positions[&new.id];
                Some(new)
            }
            After::Kept(_) => None,
        })
        .collect();
    insert_words(connection, written).await
}

/// Carries the edits that stand in meeting `from` into meeting `into`, as change `event_id` of
/// `into`, as a merge does: each edit's words written again, `shift_ms` later on `into`'s clock,
/// after every word `into` has, together with the words of its capture it replaced, written
/// already replaced by `event_id`, so a rebuild of `into` knows which capture the edit belongs to
/// and moves it wherever that capture goes. Each run has an `edit` mark of its own made by whoever
/// made the edit. Answers the edit words written.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn carry_edits(
    connection: &mut AsyncPgConnection,
    (from, into): (Uuid, Uuid),
    shift_ms: i64,
    event_id: Uuid,
) -> QueryResult<Vec<Uuid>> {
    let every_word: Vec<Word> = meeting_words::table
        .filter(meeting_words::meeting_id.eq(from))
        .order(meeting_words::position.asc())
        .select(Word::as_select())
        .load(connection)
        .await?;
    // Only a person's edits are carried: a merge settles nothing again.
    let runs = edit_runs(&every_word, Overwriting::Edits);
    if runs.is_empty() {
        return Ok(Vec::new());
    }
    let after: Option<i64> = meeting_words::table
        .filter(meeting_words::meeting_id.eq(into))
        .select(diesel::dsl::max(meeting_words::position))
        .first(connection)
        .await?;
    let mut position = after.unwrap_or(0);

    // Each edit's word and each word an edit replaced, in their order, written again in `into`.
    let covering = covering_edits(&every_word, Overwriting::Edits);
    let mut carried: HashMap<Uuid, Uuid> = HashMap::new();
    let mut covered_copies = Vec::new();
    let mut rows = Vec::new();
    for word in &every_word {
        let stands_as_edit = word.superseded_by.is_none() && word.origin == Origin::Edit.as_str();
        let covered = is_covered(word, &covering);
        if !stands_as_edit && !covered {
            continue;
        }
        position += POSITION_GAP;
        let copy = NewWord {
            id: Uuid::now_v7(),
            meeting_id: into,
            position,
            text: word.text.clone(),
            speaker: word.speaker.clone(),
            start_ms: word.start_ms + shift_ms,
            end_ms: word.end_ms + shift_ms,
            origin: word.origin.clone(),
            capture_id: word.capture_id,
            introduced_by: event_id,
        };
        carried.insert(word.id, copy.id);
        if covered {
            covered_copies.push(copy.id);
        }
        rows.push(copy);
    }
    insert_words(connection, rows).await?;
    supersede(connection, &covered_copies, event_id).await?;

    // The edits that wrote them, read once: each run's mark names whoever made its edit.
    let introduced_by: Vec<Uuid> = runs.iter().map(|run| run[0].introduced_by).collect();
    let edits: HashMap<Uuid, MeetingEvent> = crate::database::schema::meeting_events::table
        .filter(crate::database::schema::meeting_events::id.eq_any(&introduced_by))
        .select(MeetingEvent::as_select())
        .load(connection)
        .await?
        .into_iter()
        .map(|edit| (edit.id, edit))
        .collect();
    let mut written = Vec::new();
    for run in runs {
        let copies: Vec<Uuid> = run.iter().map(|word| carried[&word.id]).collect();
        let onto = Range {
            start_word_id: copies[0],
            end_word_id: copies[copies.len() - 1],
        };
        written.extend(copies);
        // The mark names whoever made the edit, as the edit's own mark did.
        let made_by = edits.get(&run[0].introduced_by);
        let author = Author {
            user_id: made_by.map_or(crate::models::user::SYSTEM_USER_ID, |edit| edit.created_by),
            agent: None,
            at: made_by.map_or_else(Utc::now, |edit| edit.created_at),
        };
        meeting_mark::insert(
            connection,
            NewMark::new(into, MarkKind::Edit, onto, json!({}), author),
        )
        .await?;
    }
    Ok(written)
}

/// Takes back the edits change `event_id` carried into meeting `meeting_id` (see [`carry_edits`]),
/// as change `undo_id`, as a split does: its words stand no longer, and their marks are removed.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn withdraw_carried(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    event_id: Uuid,
    undo_id: Uuid,
) -> QueryResult<()> {
    let carried: Vec<Uuid> = meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::introduced_by.eq(event_id))
        .select(meeting_words::id)
        .load(connection)
        .await?;
    if carried.is_empty() {
        return Ok(());
    }
    supersede(connection, &carried, undo_id).await?;
    let marks: Vec<Uuid> = meeting_mark::anchored_in(connection, meeting_id, &carried)
        .await?
        .into_iter()
        .filter(|mark| mark.kind() == Some(MarkKind::Edit))
        .map(|mark| mark.id)
        .collect();
    meeting_mark::remove_all(connection, &marks, undo_id).await?;
    Ok(())
}

/// Whether meeting `meeting_id` has a final transcript.
///
/// # Errors
/// Propagates any database error.
pub async fn has_words(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        meeting_words::table.filter(meeting_words::meeting_id.eq(meeting_id)),
    ))
    .get_result(connection)
    .await
}

/// How many words meeting `meeting_id`'s final transcript reads.
///
/// # Errors
/// Propagates any database error.
pub async fn standing_count(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<usize> {
    let count: i64 = meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::superseded_by.is_null())
        .count()
        .get_result(connection)
        .await?;
    Ok(usize::try_from(count).unwrap_or(0))
}

/// Meeting `meeting_id`'s final transcript as it reads: its standing words, in order.
///
/// # Errors
/// Propagates any database error.
pub async fn standing(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<Word>> {
    meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::superseded_by.is_null())
        .order(meeting_words::position.asc())
        .select(Word::as_select())
        .load(connection)
        .await
}

/// The words of meeting `meeting_id` among `ids`, standing or replaced, in order.
///
/// # Errors
/// Propagates any database error.
pub async fn find_many(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    ids: &[Uuid],
) -> QueryResult<Vec<Word>> {
    meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::id.eq_any(ids))
        .order(meeting_words::position.asc())
        .select(Word::as_select())
        .load(connection)
        .await
}

/// A transcript's words read as text, a space between each.
pub fn text_of(words: &[Word]) -> String {
    words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// What one change did to a meeting's words, as its history entry (`edited`) records it, so it can
/// be undone exactly. An undo is a change of the same shape, its inverse.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordChange {
    /// The words it replaced, in order, which stand no longer.
    pub replaced: Vec<Uuid>,
    /// The words it stood in their place, in order: written by it, or put back by an undo.
    pub words: Vec<Uuid>,
    /// Every mark it moved, with its range before and after.
    pub marks: Vec<MovedMark>,
    /// The marks it made or put back: an edit's own mark.
    pub marks_added: Vec<Uuid>,
    /// The marks it removed: an undone edit's own mark.
    pub marks_removed: Vec<Uuid>,
    /// What the replaced words said, and what their replacement says.
    pub before_text: String,
    pub after_text: String,
}

/// One mark a change moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedMark {
    pub id: Uuid,
    pub before: Range,
    pub after: Range,
}

/// An edit: the range of standing words it replaces, and what it writes in their place.
#[derive(Debug, Clone, Copy)]
pub struct Replacement<'text> {
    pub range: Range,
    /// Split at whitespace into the new words.
    pub text: &'text str,
}

/// Why an edit could not be made.
#[derive(Debug, thiserror::Error)]
pub enum EditError {
    /// The meeting, or a word of the range, is not the meeting's.
    #[error("not found")]
    NotFound,
    /// A word of the range was replaced already.
    #[error("a word of the range was replaced")]
    NotStanding,
    /// The range ends before it starts.
    #[error("the range ends before it starts")]
    OutOfOrder,
    /// The edit writes no words. Leaving words out of what matters is a black-out's work.
    #[error("an edit writes at least one word")]
    Empty,
    /// The edit writes more than [`EDIT_TEXT_MAX_CHARACTERS`].
    #[error("an edit writes at most {EDIT_TEXT_MAX_CHARACTERS} characters")]
    TooLong,
    /// The meeting was merged into another, which is where it changes now.
    #[error("the meeting was merged into another")]
    Merged,
    /// The meeting is deleted, so it changes no further until it is restored.
    #[error("the meeting is deleted")]
    Deleted,
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
}

/// Replaces the standing words of `replacement.range` in meeting `meeting_id` with
/// `replacement.text`'s words, as `author`, and records it. The new words take the replaced
/// words' span of time, spread evenly, and the first replaced word's speaker. Every mark over the
/// replaced words moves onto the new ones, and the edit makes a mark of its own over them (the
/// module's docs). Answers the history entry.
///
/// # Errors
/// [`EditError::NotFound`], [`EditError::NotStanding`], or [`EditError::OutOfOrder`] for a range
/// that is not one of the transcript as it reads, [`EditError::Empty`] or
/// [`EditError::TooLong`] for the text, [`EditError::Merged`] for a meeting merged into another,
/// [`EditError::Deleted`] for one deleted; any database error.
pub async fn edit(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    replacement: Replacement<'_>,
    author: Author<'_>,
) -> Result<MeetingEvent, EditError> {
    if replacement.text.chars().count() > EDIT_TEXT_MAX_CHARACTERS {
        return Err(EditError::TooLong);
    }
    let texts: Vec<&str> = replacement.text.split_whitespace().collect();
    if texts.is_empty() {
        return Err(EditError::Empty);
    }
    connection
        .transaction(async |connection| {
            match meeting::lock(connection, meeting_id).await {
                // Read under the lock a merge takes, so no edit lands on a meeting merged away.
                Ok(locked) if locked.merged_into.is_some() => return Err(EditError::Merged),
                // And under the lock a deletion takes, so none lands on a meeting deleted.
                Ok(locked) if locked.deleted_at.is_some() => return Err(EditError::Deleted),
                Ok(_) => {}
                Err(diesel::result::Error::NotFound) => return Err(EditError::NotFound),
                Err(error) => return Err(EditError::Database(error)),
            }
            let range = replacement.range;
            let (first, last) = standing_ends(connection, meeting_id, range).await?;
            let replaced = standing_between(connection, meeting_id, &first, &last).await?;
            let event_id = Uuid::now_v7();
            let replacing =
                plan_replacement(connection, &replaced, &texts, Origin::Edit, event_id).await?;
            let edit_mark = NewMark::new(
                meeting_id,
                MarkKind::Edit,
                replacing.onto(),
                json!({}),
                author,
            );
            let change = replacing.change(vec![edit_mark.id]);

            let entry =
                record_change(connection, meeting_id, event_id, &change, author, None).await?;
            replacing.write(connection).await?;
            meeting_mark::insert(connection, edit_mark).await?;
            Ok(entry)
        })
        .await
}

/// New words about to stand in place of a range of standing words, as an edit or a correction
/// writes them, and the marks that move onto them.
#[derive(Debug)]
struct Replacing {
    replaced: Vec<Uuid>,
    words: Vec<NewWord>,
    moves: Vec<MovedMark>,
    before_text: String,
    after_text: String,
}

impl Replacing {
    /// The range of its new words.
    fn onto(&self) -> Range {
        Range {
            start_word_id: self.words[0].id,
            end_word_id: self.words[self.words.len() - 1].id,
        }
    }

    /// What it does, as a history entry records it, with `marks_added` made over its words.
    fn change(&self, marks_added: Vec<Uuid>) -> WordChange {
        WordChange {
            replaced: self.replaced.clone(),
            words: self.words.iter().map(|word| word.id).collect(),
            marks: self.moves.clone(),
            marks_added,
            marks_removed: Vec::new(),
            before_text: self.before_text.clone(),
            after_text: self.after_text.clone(),
        }
    }

    /// Writes it: the replaced words stand no longer, the new ones stand in their place, and the
    /// marks over the old words cover the new ones. The change it is part of is recorded first,
    /// since every word it writes names it.
    async fn write(self, connection: &mut AsyncPgConnection) -> QueryResult<()> {
        let event_id = self.words[0].introduced_by;
        supersede(connection, &self.replaced, event_id).await?;
        insert_words(connection, self.words).await?;
        move_marks(connection, &self.moves).await
    }
}

/// What writing `texts` in place of `replaced`, a run of standing words in order, takes as change
/// `event_id`: the new words just after the last replaced one, sharing the replaced words' span of
/// time, spread evenly, with the first replaced word's speaker, each of `origin`. A correction's
/// words keep the capture of the words they correct; an edit's were said by no capture. Every mark
/// over the replaced words moves onto the new ones.
async fn plan_replacement(
    connection: &mut AsyncPgConnection,
    replaced: &[Word],
    texts: &[&str],
    origin: Origin,
    event_id: Uuid,
) -> QueryResult<Replacing> {
    let (Some(first), Some(last)) = (replaced.first(), replaced.last()) else {
        panic!("a replacement replaces at least one standing word");
    };
    let meeting_id = first.meeting_id;
    let positions = room_after(connection, meeting_id, last.id, texts.len()).await?;
    let start_ms = first.start_ms;
    let end_ms = last.end_ms.max(start_ms);
    let count = i64::try_from(texts.len()).expect("a replacement's words fit a 64-bit count");
    let capture_id = match origin {
        Origin::Edit => None,
        Origin::Source | Origin::Whisper | Origin::Correction => first.capture_id,
    };
    let words: Vec<NewWord> = texts
        .iter()
        .zip(positions)
        .zip(0_i64..)
        .map(|((text, position), index)| NewWord {
            id: Uuid::now_v7(),
            meeting_id,
            position,
            text: (*text).to_owned(),
            speaker: first.speaker.clone(),
            start_ms: start_ms + (end_ms - start_ms) * index / count,
            end_ms: start_ms + (end_ms - start_ms) * (index + 1) / count,
            origin: origin.as_str().to_owned(),
            capture_id,
            introduced_by: event_id,
        })
        .collect();
    let replaced_ids: Vec<Uuid> = replaced.iter().map(|word| word.id).collect();
    let onto = Range {
        start_word_id: words[0].id,
        end_word_id: words[words.len() - 1].id,
    };
    let anchored = meeting_mark::anchored_in(connection, meeting_id, &replaced_ids).await?;
    let moves = moves_onto(&anchored, &replaced_ids, onto, &HashMap::new());
    Ok(Replacing {
        replaced: replaced_ids,
        words,
        moves,
        before_text: text_of(replaced),
        after_text: texts.join(" "),
    })
}

/// One range a correction settles: the standing words it covers, in order, as the correction read
/// them; the text it decided on, when that differs from theirs; and its confidence mark's data,
/// `{ confidence, alternatives }`.
#[derive(Debug, Clone, PartialEq)]
pub struct SettledRange {
    pub words: Vec<Uuid>,
    pub text: Option<String>,
    pub mark: Value,
}

/// What [`settle`] did.
#[derive(Debug, Clone)]
pub struct Settled {
    /// Its history entry, `settled`.
    pub entry: MeetingEvent,
    /// For each range, in the order given, what its mark covers: its new words, or the words it
    /// left as they were; `None` for a range passed over.
    pub marked: Vec<Option<Range>>,
}

/// What a `settled` history entry records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettledChange {
    /// Each range whose text it changed, as an edit records its own.
    pub changes: Vec<WordChange>,
    /// Every confidence mark it made.
    pub marks_added: Vec<Uuid>,
    /// How many ranges it passed over because their words changed since they were read.
    pub skipped: usize,
}

/// Settles meeting `meeting_id`'s final transcript as `author`, under its lock, as one change of
/// its history: over each of `ranges` the text it decided on replaces the range's words as words of
/// the correction's own, and a confidence mark covers the range. A range whose words no longer
/// stand exactly as the correction read them is passed over, since whatever changed them is newer
/// than the correction's reading, and so is one sharing a word with a range before it.
///
/// # Errors
/// Propagates any database error, such as a meeting that does not exist.
///
/// # Panics
/// When a range names no words, or a text no words: the correction's checks keep both out.
pub async fn settle(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    ranges: &[SettledRange],
    author: Author<'_>,
) -> QueryResult<Settled> {
    connection
        .transaction(async |connection| {
            meeting::lock(connection, meeting_id).await?;
            // Recorded first, since every word and mark it writes names it; what it did is filled
            // in once it is done.
            let event_id = Uuid::now_v7();
            meeting_event::record(
                connection,
                Change {
                    id: event_id,
                    meeting_id,
                    kind: EventKind::Settled,
                    author,
                    data: json!({}),
                    undid: None,
                },
            )
            .await?;

            let mut recorded = SettledChange {
                changes: Vec::new(),
                marks_added: Vec::new(),
                skipped: 0,
            };
            // Every range is checked against the transcript as it reads now, read once. Writing one
            // range leaves every other's words standing in their order only while no two share a
            // word, which the correction's checks keep so and `claimed` makes sure of.
            let standing_now = standing(connection, meeting_id).await?;
            let place: HashMap<Uuid, usize> = standing_now
                .iter()
                .enumerate()
                .map(|(index, word)| (word.id, index))
                .collect();
            let mut marked = Vec::with_capacity(ranges.len());
            let mut marks_alone: Vec<NewMark> = Vec::new();
            let mut claimed: HashSet<Uuid> = HashSet::new();
            for settled in ranges {
                let (Some(first), Some(last)) =
                    (settled.words.as_slice().first(), settled.words.last())
                else {
                    panic!("a settled range covers at least one word");
                };
                let ends = Range {
                    start_word_id: *first,
                    end_word_id: *last,
                };
                let words = place
                    .get(first)
                    .and_then(|start| standing_now.get(*start..*start + settled.words.len()))
                    .filter(|words| {
                        words
                            .iter()
                            .map(|word| word.id)
                            .eq(settled.words.iter().copied())
                    })
                    .filter(|words| words.iter().all(|word| !claimed.contains(&word.id)));
                let Some(words) = words else {
                    recorded.skipped += 1;
                    marked.push(None);
                    continue;
                };
                claimed.extend(settled.words.iter().copied());
                let Some(text) = settled.text.as_deref() else {
                    // Its words stand as they are; only its mark is new, written with the rest.
                    let mark = NewMark::new(
                        meeting_id,
                        MarkKind::Confidence,
                        ends,
                        settled.mark.clone(),
                        author,
                    );
                    recorded.marks_added.push(mark.id);
                    marks_alone.push(mark);
                    marked.push(Some(ends));
                    continue;
                };
                let texts: Vec<&str> = text.split_whitespace().collect();
                assert!(!texts.is_empty(), "a settled text writes at least one word");
                let replacing =
                    plan_replacement(connection, words, &texts, Origin::Correction, event_id)
                        .await?;
                let onto = replacing.onto();
                let mark = NewMark::new(
                    meeting_id,
                    MarkKind::Confidence,
                    onto,
                    settled.mark.clone(),
                    author,
                );
                recorded.changes.push(replacing.change(vec![mark.id]));
                recorded.marks_added.push(mark.id);
                replacing.write(connection).await?;
                meeting_mark::insert(connection, mark).await?;
                marked.push(Some(onto));
            }

            meeting_mark::insert_all(connection, marks_alone).await?;

            let data = serde_json::to_value(&recorded).expect("a settlement is plain fields");
            let entry = meeting_event::set_data(connection, event_id, data).await?;
            Ok(Settled { entry, marked })
        })
        .await
}

/// Undoes `undone`, an `edited` entry, as `author`: the words it replaced stand again, its words
/// are replaced, the marks it moved go back where they were, and its own mark goes. The undo is
/// an entry of the same shape, so it can be undone in turn. Answers `None`, changing nothing, when
/// its words moved on since, another change having replaced one of them or put back what it
/// replaced, so undoing it would overwrite that. The caller holds the meeting's lock.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn undo(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    author: Author<'_>,
) -> QueryResult<Option<MeetingEvent>> {
    let change: WordChange = match serde_json::from_value(undone.data.clone()) {
        Ok(change) => change,
        Err(error) => {
            event!(
                name: "meetings.history.unreadable",
                Level::WARN,
                meeting_event.id = %undone.id,
                error.message = %error,
                "an edit's history entry is not in its shape; it cannot be undone",
            );
            return Ok(None);
        }
    };
    let meeting_id = undone.meeting_id;
    if !as_it_left_them(connection, undone, &change).await? {
        return Ok(None);
    }
    let (Some(first), Some(last)) = (change.replaced.as_slice().first(), change.replaced.last())
    else {
        // Only an entry written by hand replaces nothing.
        return Ok(None);
    };
    let onto = Range {
        start_word_id: *first,
        end_word_id: *last,
    };

    let event_id = Uuid::now_v7();
    let anchored = meeting_mark::anchored_in(connection, meeting_id, &change.words).await?;
    let exact: HashMap<Uuid, MovedMark> = change
        .marks
        .iter()
        .map(|moved| (moved.id, *moved))
        .collect();
    let moves = moves_onto(&anchored, &change.words, onto, &exact);
    let (to_remove, to_restore) = its_own_marks(connection, undone, &change).await?;
    let inverse = WordChange {
        replaced: change.words.clone(),
        words: change.replaced.clone(),
        marks: moves.clone(),
        marks_added: to_restore.clone(),
        marks_removed: to_remove.clone(),
        before_text: change.after_text,
        after_text: change.before_text,
    };

    let entry = record_change(
        connection,
        meeting_id,
        event_id,
        &inverse,
        author,
        Some(undone.id),
    )
    .await?;
    supersede(connection, &change.words, event_id).await?;
    diesel::update(
        meeting_words::table
            .filter(meeting_words::id.eq_any(&change.replaced))
            .filter(meeting_words::superseded_by.eq(undone.id)),
    )
    .set(meeting_words::superseded_by.eq(None::<Uuid>))
    .execute(connection)
    .await?;
    move_marks(connection, &moves).await?;
    meeting_mark::remove_all(connection, &to_remove, event_id).await?;
    meeting_mark::restore_all(connection, &to_restore, undone.id).await?;
    Ok(Some(entry))
}

/// Whether `change`, which `undone` recorded, still holds: every word it stood stands, and every
/// word it replaced is still replaced by it.
async fn as_it_left_them(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    change: &WordChange,
) -> QueryResult<bool> {
    let named: Vec<Uuid> = change
        .words
        .iter()
        .chain(&change.replaced)
        .copied()
        .collect();
    let words: HashMap<Uuid, Word> = find_many(connection, undone.meeting_id, &named)
        .await?
        .into_iter()
        .map(|word| (word.id, word))
        .collect();
    let its_words_stand = change.words.iter().all(|id| {
        words
            .get(id)
            .is_some_and(|word| word.superseded_by.is_none())
    });
    let what_it_replaced_is_still_replaced_by_it = change.replaced.iter().all(|id| {
        words
            .get(id)
            .is_some_and(|word| word.superseded_by == Some(undone.id))
    });
    Ok(its_words_stand && what_it_replaced_is_still_replaced_by_it)
}

/// The marks `change`, which `undone` recorded, made or put back that still stand, which its undo
/// removes, and the marks it removed that are still removed by it, which its undo puts back.
async fn its_own_marks(
    connection: &mut AsyncPgConnection,
    undone: &MeetingEvent,
    change: &WordChange,
) -> QueryResult<(Vec<Uuid>, Vec<Uuid>)> {
    let named: Vec<Uuid> = change
        .marks_added
        .iter()
        .chain(&change.marks_removed)
        .copied()
        .collect();
    let marks: HashMap<Uuid, Mark> = meeting_marks::table
        .filter(meeting_marks::meeting_id.eq(undone.meeting_id))
        .filter(meeting_marks::id.eq_any(&named))
        .select(Mark::as_select())
        .load(connection)
        .await?
        .into_iter()
        .map(|mark| (mark.id, mark))
        .collect();
    let to_remove = change
        .marks_added
        .iter()
        .filter(|id| marks.get(id).is_some_and(|mark| mark.removed_by.is_none()))
        .copied()
        .collect();
    let to_restore = change
        .marks_removed
        .iter()
        .filter(|id| {
            marks
                .get(id)
                .is_some_and(|mark| mark.removed_by == Some(undone.id))
        })
        .copied()
        .collect();
    Ok((to_remove, to_restore))
}

/// Where each mark of `anchored` goes when the words `replaced` give way to `onto`: a mark the
/// change being undone moved, and that nothing moved since, goes back exactly where it was
/// (`exact`, by mark); any other end among `replaced` moves to the matching end of `onto`. Marks
/// left where they are are not listed.
fn moves_onto(
    anchored: &[Mark],
    replaced: &[Uuid],
    onto: Range,
    exact: &HashMap<Uuid, MovedMark>,
) -> Vec<MovedMark> {
    let replaced: HashSet<&Uuid> = replaced.iter().collect();
    let mut moves = Vec::new();
    for mark in anchored {
        let current = mark.range();
        let moved_to = match exact.get(&mark.id) {
            Some(recorded) if recorded.after == current => recorded.before,
            _ => Range {
                start_word_id: if replaced.contains(&current.start_word_id) {
                    onto.start_word_id
                } else {
                    current.start_word_id
                },
                end_word_id: if replaced.contains(&current.end_word_id) {
                    onto.end_word_id
                } else {
                    current.end_word_id
                },
            },
        };
        if moved_to != current {
            moves.push(MovedMark {
                id: mark.id,
                before: current,
                after: moved_to,
            });
        }
    }
    moves
}

/// The first and last word of `range`, both standing words of the meeting, in order.
async fn standing_ends(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    range: Range,
) -> Result<(Word, Word), EditError> {
    let ends = find_many(
        connection,
        meeting_id,
        &[range.start_word_id, range.end_word_id],
    )
    .await?;
    let first = ends.iter().find(|word| word.id == range.start_word_id);
    let last = ends.iter().find(|word| word.id == range.end_word_id);
    let (Some(first), Some(last)) = (first, last) else {
        return Err(EditError::NotFound);
    };
    if first.superseded_by.is_some() || last.superseded_by.is_some() {
        return Err(EditError::NotStanding);
    }
    if last.position < first.position {
        return Err(EditError::OutOfOrder);
    }
    Ok((first.clone(), last.clone()))
}

/// The standing words from `first` to `last`, in order.
async fn standing_between(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    first: &Word,
    last: &Word,
) -> QueryResult<Vec<Word>> {
    meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::superseded_by.is_null())
        .filter(meeting_words::position.between(first.position, last.position))
        .order(meeting_words::position.asc())
        .select(Word::as_select())
        .load(connection)
        .await
}

/// `count` free positions just after word `after_id` and before whatever word comes next,
/// standing or replaced, renumbering the meeting's words first when the gap there is too small.
async fn room_after(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    after_id: Uuid,
    count: usize,
) -> QueryResult<Vec<i64>> {
    let wanted = i64::try_from(count).expect("an edit's words fit a 64-bit count");
    let after = position_of(connection, after_id).await?;
    let next: Option<i64> = meeting_words::table
        .filter(meeting_words::meeting_id.eq(meeting_id))
        .filter(meeting_words::position.gt(after))
        .select(diesel::dsl::min(meeting_words::position))
        .first(connection)
        .await?;
    let (after, step) = match next {
        None => (after, POSITION_GAP),
        Some(next) if next - after > wanted => (after, (next - after) / (wanted + 1)),
        Some(_) => {
            renumber(connection, meeting_id, after, wanted).await?;
            (position_of(connection, after_id).await?, POSITION_GAP)
        }
    };
    Ok((1..=wanted).map(|index| after + step * index).collect())
}

async fn position_of(connection: &mut AsyncPgConnection, word_id: Uuid) -> QueryResult<i64> {
    meeting_words::table
        .find(word_id)
        .select(meeting_words::position)
        .first(connection)
        .await
}

/// Renumbers every word of meeting `meeting_id`, standing or replaced, [`POSITION_GAP`] apart in
/// the order they hold, leaving `wanted` gaps more after the word at position `after`, so an edit
/// of that many words fits there whatever its length. One statement, which
/// `meeting_words_order`, deferrable, checks once at its end rather than row by row.
async fn renumber(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    after: i64,
    wanted: i64,
) -> QueryResult<()> {
    diesel::sql_query(
        "UPDATE meeting_words \
         SET position = (ranked.number + CASE WHEN ranked.position > $2 THEN $3 ELSE 0 END) * $4 \
         FROM (SELECT id, position, row_number() OVER (ORDER BY position) AS number \
               FROM meeting_words WHERE meeting_id = $1) AS ranked \
         WHERE meeting_words.id = ranked.id",
    )
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<BigInt, _>(after)
    .bind::<BigInt, _>(wanted)
    .bind::<BigInt, _>(POSITION_GAP)
    .execute(connection)
    .await?;
    event!(
        name: "meetings.transcript.renumbered",
        Level::DEBUG,
        meeting.id = %meeting_id,
        "an edit found no room between two words, so the meeting's words were renumbered",
    );
    Ok(())
}

/// Marks the standing words among `ids` as replaced by change `event_id`.
async fn supersede(
    connection: &mut AsyncPgConnection,
    ids: &[Uuid],
    event_id: Uuid,
) -> QueryResult<()> {
    diesel::update(
        meeting_words::table
            .filter(meeting_words::id.eq_any(ids))
            .filter(meeting_words::superseded_by.is_null()),
    )
    .set(meeting_words::superseded_by.eq(event_id))
    .execute(connection)
    .await
    .map(drop)
}

async fn move_marks(connection: &mut AsyncPgConnection, moves: &[MovedMark]) -> QueryResult<()> {
    for moved in moves {
        meeting_mark::set_range(connection, moved.id, moved.after).await?;
    }
    Ok(())
}

async fn insert_words(connection: &mut AsyncPgConnection, rows: Vec<NewWord>) -> QueryResult<()> {
    for chunk in rows.chunks(INSERT_CHUNK) {
        diesel::insert_into(meeting_words::table)
            .values(chunk)
            .execute(connection)
            .await?;
    }
    Ok(())
}

/// Records `change` as entry `event_id` of meeting `meeting_id`'s history.
async fn record_change(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    event_id: Uuid,
    change: &WordChange,
    author: Author<'_>,
    undid: Option<Uuid>,
) -> QueryResult<MeetingEvent> {
    let data: Value = serde_json::to_value(change).expect("a word change is plain fields");
    meeting_event::record(
        connection,
        Change {
            id: event_id,
            meeting_id,
            kind: EventKind::Edited,
            author,
            data,
            undid,
        },
    )
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use diesel_async::SimpleAsyncConnection;
    use serde_json::json;

    use super::*;
    use crate::meetings::final_transcript;
    use crate::meetings::history::{self, UndoError};
    use crate::meetings::source::TranscriptLine;
    use crate::models::meeting::tests::fetched_with;
    use crate::models::meeting_capture::MeetingCapture;
    use crate::models::meeting_mark::MarkError;
    use crate::models::user::SYSTEM_USER_ID;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    pub(crate) fn person() -> Author<'static> {
        Author {
            user_id: TEST_PERSON_ID,
            agent: None,
            at: Utc::now(),
        }
    }

    /// A meeting whose final transcript was built, as its source heard it, from `lines`, each
    /// `(speaker, text)` two seconds long, one after another.
    pub(crate) async fn built_meeting(
        connection: &mut AsyncPgConnection,
        lines: &[(&str, &str)],
    ) -> (MeetingCapture, Vec<Word>) {
        let heard: Vec<TranscriptLine> = lines
            .iter()
            .zip(0_i64..)
            .map(|((speaker, text), number)| TranscriptLine {
                start_ms: number * 2_000,
                end_ms: number * 2_000 + 2_000,
                speaker: (*speaker).to_owned(),
                text: (*text).to_owned(),
            })
            .collect();
        let (meeting, capture) = fetched_with(
            connection,
            serde_json::to_value(&heard).expect("serializes"),
        )
        .await;
        final_transcript::rebuild(connection, meeting.id, system())
            .await
            .expect("builds")
            .expect("a transcript");
        let words = standing(connection, meeting.id).await.expect("reads");
        (capture, words)
    }

    /// Elysium, building a transcript as a meeting's run does.
    pub(crate) fn system() -> Author<'static> {
        Author {
            user_id: SYSTEM_USER_ID,
            agent: None,
            at: Utc::now(),
        }
    }

    /// The word of `words` that reads `text`.
    pub(crate) fn word<'words>(words: &'words [Word], text: &str) -> &'words Word {
        words
            .iter()
            .find(|word| word.text == text)
            .unwrap_or_else(|| panic!("no word {text:?} in {words:?}"))
    }

    pub(crate) fn range(first: &Word, last: &Word) -> Range {
        Range {
            start_word_id: first.id,
            end_word_id: last.id,
        }
    }

    async fn text(connection: &mut AsyncPgConnection, meeting_id: Uuid) -> String {
        text_of(&standing(connection, meeting_id).await.expect("reads"))
    }

    async fn mark(connection: &mut AsyncPgConnection, meeting_id: Uuid, id: Uuid) -> Mark {
        meeting_mark::find(connection, meeting_id, id)
            .await
            .expect("the mark")
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_transcript_is_built_with_every_word_naming_its_build() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(
            &mut connection,
            &[("Ada", "Launch is Friday."), ("Grace", "Agreed.")],
        )
        .await;
        assert_eq!(text_of(&words), "Launch is Friday. Agreed.");
        assert_eq!(
            words
                .iter()
                .map(|word| (word.speaker.as_str(), word.position))
                .collect::<Vec<_>>(),
            vec![("Ada", 1024), ("Ada", 2048), ("Ada", 3072), ("Grace", 4096)]
        );
        let history = meeting_event::history(&mut connection, capture.meeting_id)
            .await
            .expect("history");
        let [built] = history.as_slice() else {
            panic!("one build, not {history:?}");
        };
        assert_eq!(built.kind(), Some(EventKind::Built));
        assert!(words.iter().all(|word| word.introduced_by == built.id));
        assert!(
            words
                .iter()
                .all(|word| word.capture_id == Some(capture.id) && word.origin == "source")
        );

        // Lining it up again from the same captures changes nothing, and keeps every word's id.
        assert!(
            final_transcript::rebuild(&mut connection, capture.meeting_id, system())
                .await
                .expect("answers")
                .is_none()
        );
        assert_eq!(
            standing(&mut connection, capture.meeting_id)
                .await
                .expect("reads"),
            words
        );
        assert!(matches!(
            history::undo(&mut connection, capture.meeting_id, built.id, person()).await,
            Err(UndoError::NotUndoable)
        ));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    #[expect(
        clippy::too_many_lines,
        reason = "two edits over three marks, undone and redone, with what each left behind"
    )]
    async fn an_edit_replaces_its_range_and_marks_over_it_cover_its_words_until_undone() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(
            &mut connection,
            &[("Ada", "Jon ships Friday"), ("Grace", "Agreed")],
        )
        .await;
        let meeting_id = capture.meeting_id;
        let (jon, ships, friday, agreed) = (
            word(&words, "Jon").clone(),
            word(&words, "ships").clone(),
            word(&words, "Friday").clone(),
            word(&words, "Agreed").clone(),
        );
        let (excerpt, _) = meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::Excerpt,
            range(&jon, &friday),
            json!({}),
            person(),
        )
        .await
        .expect("an excerpt");
        let (comment, _) = meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::Comment,
            range(&ships, &ships),
            json!({}),
            person(),
        )
        .await
        .expect("a comment");
        let (black_out, _) = meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::BlackOut,
            range(&friday, &agreed),
            json!({}),
            person(),
        )
        .await
        .expect("a black-out");

        // A name heard wrong: one word becomes two, in the span the old word took.
        let renamed = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&jon, &jon),
                text: " John  Smith ",
            },
            person(),
        )
        .await
        .expect("edits");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith ships Friday Agreed"
        );
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        let (first_name, surname) = (word(&standing_now, "John"), word(&standing_now, "Smith"));
        assert_eq!(
            (
                first_name.start_ms,
                first_name.end_ms,
                surname.start_ms,
                surname.end_ms
            ),
            (jon.start_ms, 333, 333, jon.end_ms),
            "the new words share the span the old word took"
        );
        assert_eq!(
            (
                first_name.origin.as_str(),
                first_name.capture_id,
                first_name.speaker.as_str()
            ),
            ("edit", None, "Ada")
        );
        assert_eq!(first_name.introduced_by, renamed.id);
        // The excerpt started on the word replaced, so it starts on the first new word now.
        assert_eq!(
            mark(&mut connection, meeting_id, excerpt.id).await.range(),
            range(first_name, &friday)
        );
        let change: WordChange = serde_json::from_value(renamed.data.clone()).expect("reads");
        assert_eq!(change.before_text, "Jon");
        assert_eq!(change.after_text, "John Smith");
        let [edit_mark] = change.marks_added.as_slice() else {
            panic!("an edit makes one mark of its own");
        };
        let own = mark(&mut connection, meeting_id, *edit_mark).await;
        assert_eq!(
            (own.kind(), own.range(), own.created_by),
            (
                Some(MarkKind::Edit),
                range(first_name, surname),
                TEST_PERSON_ID
            )
        );

        // A second edit across the comment's word and the black-out's start.
        let reworded = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&ships, &friday),
                text: "is shipping Monday",
            },
            person(),
        )
        .await
        .expect("edits");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith is shipping Monday Agreed"
        );
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        let (is, monday) = (word(&standing_now, "is"), word(&standing_now, "Monday"));
        assert_eq!(
            mark(&mut connection, meeting_id, comment.id).await.range(),
            range(is, monday),
            "a mark inside the range covers all of its new words"
        );
        assert_eq!(
            mark(&mut connection, meeting_id, black_out.id)
                .await
                .range(),
            range(is, &agreed)
        );
        assert_eq!(
            mark(&mut connection, meeting_id, excerpt.id).await.range(),
            range(word(&standing_now, "John"), monday)
        );

        // Undoing the second puts its words and marks back exactly.
        let undone = history::undo(&mut connection, meeting_id, reworded.id, person())
            .await
            .expect("undoes")
            .entry;
        assert_eq!(undone.kind(), Some(EventKind::Edited));
        assert_eq!(undone.undid(), Some(reworded.id));
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith ships Friday Agreed"
        );
        assert_eq!(
            mark(&mut connection, meeting_id, comment.id).await.range(),
            range(&ships, &ships)
        );
        assert_eq!(
            mark(&mut connection, meeting_id, black_out.id)
                .await
                .range(),
            range(&friday, &agreed)
        );
        let second: WordChange = serde_json::from_value(reworded.data.clone()).expect("reads");
        let second_mark = mark(&mut connection, meeting_id, second.marks_added[0]).await;
        assert_eq!(
            second_mark.removed_by,
            Some(undone.id),
            "its own mark goes with it"
        );
        // A change is undone once; again, its words are gone.
        assert!(matches!(
            history::undo(&mut connection, meeting_id, reworded.id, person()).await,
            Err(UndoError::ChangedSince)
        ));

        // Undoing the undo does the edit again, its mark back.
        history::undo(&mut connection, meeting_id, undone.id, person())
            .await
            .expect("redoes");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John Smith is shipping Monday Agreed"
        );
        assert_eq!(
            mark(&mut connection, meeting_id, second_mark.id)
                .await
                .removed_by,
            None
        );

        // The first edit's words were not replaced since, so it undoes too, under the second.
        history::undo(&mut connection, meeting_id, renamed.id, person())
            .await
            .expect("undoes");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "Jon is shipping Monday Agreed"
        );
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        assert_eq!(
            mark(&mut connection, meeting_id, excerpt.id).await.range(),
            range(&jon, word(&standing_now, "Monday"))
        );
        // Every mark stands on standing words, removed ones too.
        let anchored: Vec<Uuid> = standing_now.iter().map(|word| word.id).collect();
        for id in [
            excerpt.id,
            comment.id,
            black_out.id,
            *edit_mark,
            second_mark.id,
        ] {
            let anchors = mark(&mut connection, meeting_id, id).await.range();
            assert!(
                anchored.contains(&anchors.start_word_id)
                    && anchored.contains(&anchors.end_word_id),
                "mark {id} stands on standing words"
            );
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_edit_whose_words_were_edited_since_cannot_be_undone() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships")]).await;
        let meeting_id = capture.meeting_id;
        let jon = word(&words, "Jon").clone();
        let first = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&jon, &jon),
                text: "John",
            },
            person(),
        )
        .await
        .expect("edits");
        let standing_now = standing(&mut connection, meeting_id).await.expect("reads");
        let corrected_name = word(&standing_now, "John").clone();
        edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&corrected_name, &corrected_name),
                text: "Johnny",
            },
            person(),
        )
        .await
        .expect("edits again");

        assert!(matches!(
            history::undo(&mut connection, meeting_id, first.id, person()).await,
            Err(UndoError::ChangedSince)
        ));
        assert_eq!(text(&mut connection, meeting_id).await, "Johnny ships");
        // A replaced word is no end of a range, for an edit or a mark.
        assert!(matches!(
            edit(
                &mut connection,
                meeting_id,
                Replacement {
                    range: range(&jon, &jon),
                    text: "Jo",
                },
                person(),
            )
            .await,
            Err(EditError::NotStanding)
        ));
        assert!(matches!(
            meeting_mark::add(
                &mut connection,
                meeting_id,
                MarkKind::Comment,
                range(&corrected_name, &corrected_name),
                json!({}),
                person(),
            )
            .await,
            Err(MarkError::NotStanding)
        ));
    }

    /// #269 review: a person's edit over a word the correction settled, carried into another meeting
    /// by a merge, still belongs to the settled word's capture there, and moves with it.
    #[test]
    fn an_edit_carried_over_a_settled_word_moves_with_the_settled_words_capture() {
        let meeting_id = Uuid::now_v7();
        let carried_by = Uuid::now_v7();
        let (heard_in, kept_capture) = (Uuid::now_v7(), Uuid::now_v7());
        let row = |text: &str, origin: Origin, capture_id: Option<Uuid>, number: i64| Word {
            id: Uuid::now_v7(),
            meeting_id,
            position: number * POSITION_GAP,
            text: text.to_owned(),
            speaker: "Ada".to_owned(),
            start_ms: 1_000,
            end_ms: 1_400,
            origin: origin.as_str().to_owned(),
            capture_id,
            introduced_by: carried_by,
            superseded_by: None,
            created_at: Utc::now(),
        };
        // As a merge writes them: the settled word the edit replaced, then the edit's own word.
        let mut settled = row("Sharath", Origin::Correction, Some(heard_in), 1);
        settled.superseded_by = Some(carried_by);
        let edited = row("Sharad", Origin::Edit, None, 2);
        let every_word = vec![settled, edited.clone()];
        let present = HashSet::from([heard_in, kept_capture]);
        let moved_by = HashMap::from([(heard_in, 500)]);

        let mut times = HashMap::new();
        let (_moved, capture_of_edit) = with_edits_moved(
            &every_word,
            &moved_by,
            Overwriting::EditsAndSettled(&present),
            &mut times,
        );

        assert_eq!(capture_of_edit.get(&edited.id), Some(&heard_in));
        assert_eq!(times.get(&edited.id), Some(&(1_500, 1_900)));
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn settled_words_stand_through_a_rebuild_while_their_capture_is_the_meetings() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(
            &mut connection,
            &[("Ada", "Jon ships"), ("Grace", "Sure thing")],
        )
        .await;
        let meeting_id = capture.meeting_id;
        let jon = word(&words, "Jon").clone();
        let sure = word(&words, "Sure").clone();
        let settled = settle(
            &mut connection,
            meeting_id,
            &[SettledRange {
                words: vec![jon.id],
                text: Some("John".to_owned()),
                mark: json!({ "confidence": 90, "alternatives": [] }),
            }],
            system(),
        )
        .await
        .expect("settles");
        assert_eq!(settled.entry.kind, "settled");
        // A person's edit elsewhere, which stands through every rebuild whatever its capture.
        edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&sure, &sure),
                text: "Certain",
            },
            person(),
        )
        .await
        .expect("edits");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John ships Certain thing"
        );

        // Lined up again from the capture, which still heard "Jon": the settled word stands.
        final_transcript::rebuild(&mut connection, meeting_id, system())
            .await
            .expect("rebuilds");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "John ships Certain thing"
        );

        // With its capture gone from the meeting, as a split takes it, the settlement goes with
        // the capture's words, and the person's edit stays.
        let gone = Rebuild {
            meeting_id,
            words: Vec::new(),
            captures: Vec::new(),
            built_from: BuiltFrom::Source,
            provisional: HashSet::new(),
        };
        rebuild(&mut connection, gone, system())
            .await
            .expect("rebuilds")
            .expect("the capture's words went");
        assert_eq!(text(&mut connection, meeting_id).await, "Certain");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_edit_is_refused_unless_it_is_a_range_of_the_transcript_and_writes_words() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships Friday")]).await;
        let (_other, other_words) = built_meeting(&mut connection, &[("Grace", "Elsewhere")]).await;
        let meeting_id = capture.meeting_id;
        let (jon, friday) = (word(&words, "Jon"), word(&words, "Friday"));

        let empty = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(jon, friday),
                text: " \t ",
            },
            person(),
        )
        .await;
        assert!(matches!(empty, Err(EditError::Empty)), "{empty:?}");
        let backwards = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(friday, jon),
                text: "x",
            },
            person(),
        )
        .await;
        assert!(
            matches!(backwards, Err(EditError::OutOfOrder)),
            "{backwards:?}"
        );
        let elsewhere = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(jon, word(&other_words, "Elsewhere")),
                text: "x",
            },
            person(),
        )
        .await;
        assert!(
            matches!(elsewhere, Err(EditError::NotFound)),
            "{elsewhere:?}"
        );
        let long = "word ".repeat(EDIT_TEXT_MAX_CHARACTERS / 5 + 1);
        let too_long = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(jon, jon),
                text: &long,
            },
            person(),
        )
        .await;
        assert!(matches!(too_long, Err(EditError::TooLong)), "{too_long:?}");
        assert_eq!(text(&mut connection, meeting_id).await, "Jon ships Friday");
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn an_edit_with_no_room_renumbers_the_words_and_keeps_every_one_in_its_place() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) =
            built_meeting(&mut connection, &[("Ada", "one two three four")]).await;
        let meeting_id = capture.meeting_id;
        // Packed tight, as a meeting edited many times over in one place leaves its words.
        connection
            .batch_execute(&format!(
                "UPDATE meeting_words SET position = position / 1024 \
                 WHERE meeting_id = '{meeting_id}'"
            ))
            .await
            .expect("packs");
        let two = word(&words, "two").clone();
        let edited = edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&two, &two),
                text: "2 and a half",
            },
            person(),
        )
        .await
        .expect("edits");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "one 2 and a half three four"
        );
        let positions: Vec<i64> = standing(&mut connection, meeting_id)
            .await
            .expect("reads")
            .iter()
            .map(|word| word.position)
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));

        history::undo(&mut connection, meeting_id, edited.id, person())
            .await
            .expect("undoes");
        assert_eq!(
            text(&mut connection, meeting_id).await,
            "one two three four"
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_meeting_removed_takes_its_words_marks_and_history_with_it() {
        let (_url, mut connection) = migrated_database().await;
        let (capture, words) = built_meeting(&mut connection, &[("Ada", "Jon ships")]).await;
        let meeting_id = capture.meeting_id;
        let jon = word(&words, "Jon").clone();
        meeting_mark::add(
            &mut connection,
            meeting_id,
            MarkKind::Comment,
            range(&jon, &jon),
            json!({}),
            person(),
        )
        .await
        .expect("a comment");
        edit(
            &mut connection,
            meeting_id,
            Replacement {
                range: range(&jon, &jon),
                text: "John",
            },
            person(),
        )
        .await
        .expect("edits");

        diesel::delete(crate::database::schema::meetings::table.find(meeting_id))
            .execute(&mut connection)
            .await
            .expect("the meeting goes, with everything that hangs from it");
        assert!(!has_words(&mut connection, meeting_id).await.expect("reads"));
        assert!(
            meeting_event::history(&mut connection, meeting_id)
                .await
                .expect("reads")
                .is_empty()
        );
    }
}
