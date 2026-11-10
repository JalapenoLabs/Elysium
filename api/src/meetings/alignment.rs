// Copyright © 2026 Jalapeno Labs

//! Putting a meeting's captures on one clock, and lining their words up into one transcript. See
//! `docs/meetings.md`, Aligning.
//!
//! Everything here is pure arithmetic over what the align step (`super::align`) and the final
//! transcript's build (`super::final_transcript`) read: recordings' energy envelopes, transcripts'
//! lines, and Whisper's words. Nothing here reads a database or a file.
//!
//! # One clock
//!
//! A capture's offset places its own clock on the meeting's. The first capture is the reference
//! every other one is aligned against, in one of two ways:
//!
//! - **Audio** ([`audio_lag`]): two recordings of one meeting carry the same speech, so their
//!   energy envelopes (how loud each ten milliseconds is) rise and fall together. The lag at which
//!   the two correlate best, by a clear margin, is how far one runs behind the other.
//! - **Words** ([`words_lag`]): a capture with no audio to correlate, such as a transcript alone,
//!   is aligned by its words. Each distinctive word it shares with the reference votes for the gap
//!   between when the two heard it, and the gap most votes agree on is the lag.
//!
//! Neither answers when it is not sure; the step then keeps the offset it had, from when the
//! sources said each capture started.
//!
//! # One transcript
//!
//! [`capture_words`] turns each capture's lines into words on the meeting's clock, each timed by
//! Whisper's word timings where Whisper heard the same word, since those are exact to the audio,
//! and spread evenly across what is left of its line where it did not. Whisper tells nobody apart,
//! so [`name_speakers`] names the speaker of each word Whisper alone heard from the sources' lines
//! at the same moment. [`line_up`] then lays the captures' words out by time: every word of the
//! first capture, and of each later one only what no capture before it heard, so a moment two
//! captures heard reads once, and a stretch only one of them caught (a recording started late, a
//! connection that dropped) is still in the transcript.

use std::collections::HashMap;

use realfft::RealFftPlanner;
use realfft::num_complex::Complex;
use similar::{Algorithm, DiffOp};
use uuid::Uuid;

use super::correction::comparable;
use super::source::TranscriptLine;
use crate::models::meeting_word::{BuiltWord, HeardWord, Origin};
use crate::whisper::WhisperWord;

/// How many frames of a recording's energy envelope a second makes: ten milliseconds each, finer
/// than any word, and two hours of audio is 720,000 frames. The align step's `ffmpeg` script
/// samples envelopes at this rate (`super::media::ENVELOPE_SCRIPT`).
pub const ENVELOPE_FRAMES_PER_SECOND: i64 = 100;

/// How long one envelope frame lasts.
const FRAME_MS: i64 = 1_000 / ENVELOPE_FRAMES_PER_SECOND;

/// The least audio two recordings must share for a lag between them to count: a minute, long
/// enough that a chance likeness of a few words or a burst of noise never wins.
const MIN_OVERLAP_FRAMES: usize = 6_000;

/// The least correlation a lag must reach, averaged over the audio the two share once each
/// envelope is put on one scale. Two recordings of one call reach 0.6 and more even through
/// different codecs; unrelated speech stays near zero.
const MIN_CORRELATION: f64 = 0.3;

/// How far the best lag must stand above the best one more than [`PEAK_WIDTH_FRAMES`] from it, so
/// audio that repeats itself (hold music, a long silence) never picks a lag by a hair.
const MIN_PROMINENCE: f64 = 0.1;

/// Two seconds: lags this close to the best one are its own peak, not a rival.
const PEAK_WIDTH_FRAMES: i64 = 200;

/// A word votes on a lag only when it is at least this long, so "the" and "and" never do.
const DISTINCTIVE_WORD_CHARACTERS: usize = 4;

/// A word votes only when each transcript says it at most this often: a word said once in each is
/// a pair that can only mean one lag, and one said fifty times casts 2,500 votes for nothing.
const DISTINCTIVE_WORD_OCCURRENCES: usize = 3;

/// Votes this close together agree: about a word's length, the slack between a source's line times
/// and when a word in it was said.
const VOTE_WINDOW_MS: i64 = 1_500;

/// The fewest agreeing votes a lag from words needs.
const MIN_VOTES: usize = 5;

/// A lag from words wins only with at least this many times the votes of the best lag more than
/// [`RIVAL_DISTANCE_MS`] from it.
const VOTE_DOMINANCE: usize = 2;

/// How far a rival lag must be from the best one to be another answer rather than the same one.
const RIVAL_DISTANCE_MS: i64 = 5_000;

/// How far outside a line's own times a Whisper word may lie and still be the same word: a
/// source times its lines to a second or so.
const ANCHOR_SLACK_MS: i64 = 2_000;

/// A later capture's word joins the transcript only where no capture before it heard anything
/// within this long: about a pause between sentences, so two captures of one moment read once.
const COVERED_MS: i64 = 2_000;

/// How far `other`'s audio runs behind `reference`'s, in milliseconds: `other`'s moment `t` is
/// `reference`'s moment `t + lag`. Both are energy envelopes, [`ENVELOPE_FRAMES_PER_SECOND`] frames
/// a second (the mean square of the audio over each frame). `None` when no lag stands out: either
/// is shorter than a minute or silent, or they are not recordings of the same audio.
pub fn audio_lag(reference: &[f32], other: &[f32]) -> Option<i64> {
    if reference.len() < MIN_OVERLAP_FRAMES || other.len() < MIN_OVERLAP_FRAMES {
        return None;
    }
    let reference = standardized(reference)?;
    let other = standardized(other)?;
    let correlation = cross_correlation(&reference, &other);

    // Lag `d` pairs `reference[t + d]` with `other[t]`; the correlation of a negative lag wraps
    // around to the end of the transform.
    let size = correlation.len();
    let reference_length = signed(reference.len());
    let other_length = signed(other.len());
    let score = |lag: i64| -> Option<f64> {
        let overlap = reference_length.min(other_length + lag) - lag.max(0);
        if overlap < signed(MIN_OVERLAP_FRAMES) {
            return None;
        }
        let index = if lag >= 0 { lag } else { signed(size) + lag };
        let index = usize::try_from(index).expect("a lag inside the transform has an index");
        Some(correlation[index] / frames(overlap))
    };

    let lags = (1 - other_length)..reference_length;
    let (best_lag, best) = lags
        .clone()
        .filter_map(|lag| score(lag).map(|value| (lag, value)))
        .max_by(|left, right| left.1.total_cmp(&right.1))?;
    let rival = lags
        .filter(|lag| (lag - best_lag).abs() > PEAK_WIDTH_FRAMES)
        .filter_map(score)
        .max_by(f64::total_cmp)
        .unwrap_or(f64::NEG_INFINITY);
    if best < MIN_CORRELATION || best - rival < MIN_PROMINENCE {
        return None;
    }
    Some(best_lag * FRAME_MS)
}

/// `envelope` on one scale: each frame's energy as a multiple of the recording's mean, compressed
/// by a logarithm so a shout counts no more than a word, then centred and scaled to unit variance.
/// `None` for a recording that is silent throughout.
fn standardized(envelope: &[f32]) -> Option<Vec<f64>> {
    let energies: Vec<f64> = envelope
        .iter()
        .map(|energy| {
            let energy = f64::from(*energy);
            if energy.is_finite() {
                energy.max(0.0)
            } else {
                0.0
            }
        })
        .collect();
    let mean = energies.iter().sum::<f64>() / frames(signed(energies.len()));
    if mean <= f64::EPSILON {
        return None;
    }
    let compressed: Vec<f64> = energies
        .iter()
        .map(|energy| (energy / mean).ln_1p())
        .collect();
    let center = compressed.iter().sum::<f64>() / frames(signed(compressed.len()));
    let variance = compressed
        .iter()
        .map(|value| (value - center).powi(2))
        .sum::<f64>()
        / frames(signed(compressed.len()));
    let deviation = variance.sqrt();
    if deviation <= f64::EPSILON {
        return None;
    }
    Some(
        compressed
            .into_iter()
            .map(|value| (value - center) / deviation)
            .collect(),
    )
}

/// The cross-correlation of `reference` and `other` at every lag, by the fast Fourier transform:
/// index `d` holds the sum over `t` of `reference[t + d] * other[t]`, a negative `d` at the end.
fn cross_correlation(reference: &[f64], other: &[f64]) -> Vec<f64> {
    let size = (reference.len() + other.len()).next_power_of_two();
    let mut planner = RealFftPlanner::<f64>::new();
    let forward = planner.plan_fft_forward(size);
    let inverse = planner.plan_fft_inverse(size);
    let spectrum = |signal: &[f64]| {
        let mut padded = signal.to_vec();
        padded.resize(size, 0.0);
        let mut transformed = forward.make_output_vec();
        forward
            .process(&mut padded, &mut transformed)
            .expect("buffers are the plan's own length");
        transformed
    };
    let reference_spectrum = spectrum(reference);
    let other_spectrum = spectrum(other);
    let mut product: Vec<Complex<f64>> = reference_spectrum
        .iter()
        .zip(&other_spectrum)
        .map(|(reference_bin, other_bin)| reference_bin * other_bin.conj())
        .collect();
    // The first and last bins of a real signal's transform are real; the product of two is too,
    // but the inverse refuses even a rounding error's imaginary part there.
    if let Some(first) = product.first_mut() {
        first.im = 0.0;
    }
    if let Some(last) = product.last_mut() {
        last.im = 0.0;
    }
    let mut correlation = inverse.make_output_vec();
    inverse
        .process(&mut product, &mut correlation)
        .expect("buffers are the plan's own length");
    // The inverse transform leaves every value multiplied by the transform's length.
    let scale = frames(signed(size));
    for value in &mut correlation {
        *value /= scale;
    }
    correlation
}

/// How far `other`'s words run behind `reference`'s, in milliseconds: `other`'s moment `t` is
/// `reference`'s moment `t + lag`, each on the clock its words are timed by. `None` when too few
/// distinctive words agree on one lag, or another lag has nearly as many.
pub fn words_lag(reference: &[BuiltWord], other: &[BuiltWord]) -> Option<i64> {
    let reference_heard = occurrences(reference);
    let other_heard = occurrences(other);
    let mut votes: Vec<i64> = Vec::new();
    for (key, other_starts) in &other_heard {
        let Some(reference_starts) = reference_heard.get(key) else {
            continue;
        };
        if reference_starts.len() > DISTINCTIVE_WORD_OCCURRENCES
            || other_starts.len() > DISTINCTIVE_WORD_OCCURRENCES
        {
            continue;
        }
        for reference_start in reference_starts {
            for other_start in other_starts {
                votes.push(reference_start - other_start);
            }
        }
    }
    votes.sort_unstable();

    // The window of votes within `VOTE_WINDOW_MS` of each other that holds the most.
    let windows = vote_windows(&votes);
    let &(best_start, best_end) = windows
        .iter()
        .max_by_key(|(start, end)| (end - start, std::cmp::Reverse(*start)))?;
    let agreeing = &votes[best_start..best_end];
    if agreeing.len() < MIN_VOTES {
        return None;
    }
    let lag = agreeing[agreeing.len() / 2];
    let rival = windows
        .iter()
        .filter(|(start, end)| (votes[(start + end) / 2] - lag).abs() > RIVAL_DISTANCE_MS)
        .map(|(start, end)| end - start)
        .max()
        .unwrap_or(0);
    if agreeing.len() < rival * VOTE_DOMINANCE {
        return None;
    }
    Some(lag)
}

/// Every window of sorted `votes` that starts at a vote and holds each vote up to
/// [`VOTE_WINDOW_MS`] after it, as the range of indices it holds.
fn vote_windows(votes: &[i64]) -> Vec<(usize, usize)> {
    let mut windows = Vec::with_capacity(votes.len());
    let mut end = 0;
    for (start, vote) in votes.iter().enumerate() {
        end = end.max(start);
        while end < votes.len() && votes[end] - vote <= VOTE_WINDOW_MS {
            end += 1;
        }
        windows.push((start, end));
    }
    windows
}

/// When each distinctive word of `words` was said, by the word as it compares.
fn occurrences(words: &[BuiltWord]) -> HashMap<String, Vec<i64>> {
    let mut heard: HashMap<String, Vec<i64>> = HashMap::new();
    for word in words {
        let key = comparable(&word.text);
        if key.chars().count() < DISTINCTIVE_WORD_CHARACTERS {
            continue;
        }
        heard.entry(key).or_default().push(word.start_ms);
    }
    heard
}

/// The words of `lines`, each with where it came from, on the meeting's clock for a capture
/// `offset_ms` from it. A word Whisper heard too, near its line's times, takes Whisper's timing
/// (`whisper`, on the capture's own clock, in the order Whisper heard them); the rest of a line's
/// words are spread evenly across the time left between the words Whisper anchored, or across the
/// whole line where it anchored none. Every word keeps its line's speaker.
pub fn capture_words<'line>(
    lines: impl IntoIterator<Item = (&'line TranscriptLine, Origin)>,
    whisper: &[WhisperWord],
    offset_ms: i64,
) -> Vec<BuiltWord> {
    let whisper_keys: Vec<String> = whisper.iter().map(|word| comparable(&word.text)).collect();
    let mut words = Vec::new();
    for (line, origin) in lines {
        let texts: Vec<&str> = line.text.split_whitespace().collect();
        if texts.is_empty() {
            continue;
        }
        let start_ms = line.start_ms.max(0);
        let end_ms = line.end_ms.max(start_ms);
        let anchors = anchors_of(&texts, (start_ms, end_ms), whisper, &whisper_keys);
        let timed = spread_around(&anchors, (start_ms, end_ms));
        for (text, (word_start, word_end)) in texts.into_iter().zip(timed) {
            words.push(BuiltWord {
                text: text.to_owned(),
                speaker: line.speaker.clone(),
                start_ms: offset_ms + word_start,
                end_ms: offset_ms + word_end,
                origin,
            });
        }
    }
    words
}

/// For each of a line's words `texts`, Whisper's timing of the same word when Whisper heard it
/// within [`ANCHOR_SLACK_MS`] of the line's `span`, matched in order, word by word.
fn anchors_of(
    texts: &[&str],
    (start_ms, end_ms): (i64, i64),
    whisper: &[WhisperWord],
    whisper_keys: &[String],
) -> Vec<Option<(i64, i64)>> {
    let mut anchors = vec![None; texts.len()];
    // Whisper's words are in the order it heard them, so both ends of the window are a search.
    let first = whisper.partition_point(|word| word.end_ms < start_ms - ANCHOR_SLACK_MS);
    let last = whisper.partition_point(|word| word.start_ms <= end_ms + ANCHOR_SLACK_MS);
    if first >= last {
        return anchors;
    }
    let keys: Vec<String> = texts.iter().map(|text| comparable(text)).collect();
    let nearby = &whisper_keys[first..last];
    for operation in similar::capture_diff_slices(Algorithm::Myers, &keys, nearby) {
        let DiffOp::Equal {
            old_index,
            new_index,
            len,
        } = operation
        else {
            continue;
        };
        for step in 0..len {
            let heard = &whisper[first + new_index + step];
            anchors[old_index + step] = Some((heard.start_ms, heard.end_ms.max(heard.start_ms)));
        }
    }
    anchors
}

/// Every word's time in a line spanning `span`: its anchor's where it has one, and for each run of
/// words without, the time between the anchors either side of the run (or the line's ends) spread
/// evenly across them. No word starts before the one before it.
fn spread_around(
    anchors: &[Option<(i64, i64)>],
    (start_ms, end_ms): (i64, i64),
) -> Vec<(i64, i64)> {
    let mut timed = Vec::with_capacity(anchors.len());
    let mut index = 0;
    while index < anchors.len() {
        if let Some(anchor) = anchors[index] {
            timed.push(anchor);
            index += 1;
            continue;
        }
        let run_end = anchors[index..]
            .iter()
            .position(Option::is_some)
            .map_or(anchors.len(), |offset| index + offset);
        let left = timed.last().map_or(start_ms, |(_start, end)| *end);
        let right = anchors
            .get(run_end)
            .copied()
            .flatten()
            .map_or(end_ms, |(start, _end)| start)
            .max(left);
        let count = signed(run_end - index);
        for number in 1..=count {
            timed.push((
                left + (right - left) * (number - 1) / count,
                left + (right - left) * number / count,
            ));
        }
        index = run_end;
    }
    let mut earliest = i64::MIN;
    for (start, end) in &mut timed {
        *start = (*start).max(earliest);
        *end = (*end).max(*start);
        earliest = *start;
    }
    timed
}

/// One stretch of a source's line on the meeting's clock, and who it says was speaking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Speaking {
    pub start_ms: i64,
    pub end_ms: i64,
    pub speaker: String,
}

/// Names the speaker of each word of `words` that names none, as Whisper's words do, from
/// `speaking`, the sources' lines on the meeting's clock in order of their start: the speaker of a
/// line under the word's middle, if one is.
pub fn name_speakers(words: &mut [BuiltWord], speaking: &[Speaking]) {
    for word in words.iter_mut().filter(|word| word.speaker.is_empty()) {
        let middle = word.start_ms + (word.end_ms - word.start_ms) / 2;
        let after = speaking.partition_point(|line| line.start_ms <= middle);
        let named = speaking[..after]
            .iter()
            .rev()
            .find(|line| line.end_ms >= middle && !line.speaker.is_empty());
        if let Some(line) = named {
            word.speaker.clone_from(&line.speaker);
        }
    }
}

/// Every capture's words in one transcript, in order of time: `captures` in the meeting's order,
/// each with its words on the meeting's clock in the order it heard them. The first capture's
/// words all stand; a later capture's word joins only where no capture before it heard anything
/// within [`COVERED_MS`]. Each capture's own words keep their order.
pub fn line_up(captures: Vec<(Uuid, Vec<BuiltWord>)>) -> Vec<HeardWord> {
    let mut covered: Vec<(i64, i64)> = Vec::new();
    let mut taken: Vec<Vec<HeardWord>> = Vec::with_capacity(captures.len());
    for (capture_id, words) in captures {
        let joining: Vec<HeardWord> = words
            .into_iter()
            .filter(|word| !is_covered(&covered, word.start_ms + (word.end_ms - word.start_ms) / 2))
            .map(|word| HeardWord { capture_id, word })
            .collect();
        covered = with_covered(covered, &joining);
        taken.push(joining);
    }

    // Merges the captures' words by time, each capture's in its own order: the earliest next word
    // of any capture goes next, the earlier capture's first on a tie.
    let total = taken.iter().map(Vec::len).sum();
    let mut lined_up = Vec::with_capacity(total);
    let mut next = vec![0_usize; taken.len()];
    while lined_up.len() < total {
        let earliest = taken
            .iter()
            .zip(&next)
            .enumerate()
            .filter_map(|(rank, (words, index))| words.get(*index).map(|word| (rank, word)))
            .min_by_key(|(rank, word)| (word.word.start_ms, *rank))
            .map(|(rank, _word)| rank)
            .expect("a capture has words left while fewer than all are lined up");
        lined_up.push(taken[earliest][next[earliest]].clone());
        next[earliest] += 1;
    }
    lined_up
}

/// Whether `moment` lies inside one of `covered`, sorted stretches that never overlap.
fn is_covered(covered: &[(i64, i64)], moment: i64) -> bool {
    let after = covered.partition_point(|(start, _end)| *start <= moment);
    after > 0 && covered[after - 1].1 >= moment
}

/// `covered` with every word of `joining` added, [`COVERED_MS`] either side, as sorted stretches
/// that never overlap.
fn with_covered(covered: Vec<(i64, i64)>, joining: &[HeardWord]) -> Vec<(i64, i64)> {
    let mut stretches = covered;
    stretches.extend(joining.iter().map(|word| {
        (
            word.word.start_ms - COVERED_MS,
            word.word.end_ms + COVERED_MS,
        )
    }));
    stretches.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::with_capacity(stretches.len());
    for (start, end) in stretches {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }
    merged
}

/// A count as a signed number, for arithmetic with times and lags.
fn signed(count: usize) -> i64 {
    i64::try_from(count).expect("a count of frames or words fits 64 bits")
}

/// A count of frames as a divisor. Counts stay far below 2^52, where `f64` stops being exact.
#[expect(
    clippy::cast_precision_loss,
    reason = "frame counts stay far below 2^52, where f64 stops being exact"
)]
fn frames(count: i64) -> f64 {
    count as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A speech-like envelope `seconds` long: syllables of random length and loudness separated
    /// by pauses, from a fixed seed, so a test is the same every run.
    fn speech(seconds: usize, seed: u64) -> Vec<f32> {
        let mut state = seed;
        let mut next = move || {
            // A 64-bit linear congruential generator; good enough to vary syllables.
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            state >> 33
        };
        let length = seconds * 100;
        let mut envelope = Vec::with_capacity(length);
        while envelope.len() < length {
            let syllable = 8 + usize::try_from(next() % 20).expect("small");
            let loudness =
                0.01 + f32::from(u16::try_from(next() % 1_000).expect("small")) / 10_000.0;
            envelope.extend(std::iter::repeat_n(loudness, syllable));
            let pause = 3 + usize::try_from(next() % 30).expect("small");
            envelope.extend(std::iter::repeat_n(0.0001, pause));
        }
        envelope.truncate(length);
        envelope
    }

    /// `envelope` as another recorder heard it: `late_frames` of its own before the meeting's
    /// audio began, a different gain, and noise of its own.
    fn recorded_late(envelope: &[f32], late_frames: usize, seed: u64) -> Vec<f32> {
        let noise = speech(envelope.len() / 100 + late_frames / 100 + 1, seed);
        let mut recorded: Vec<f32> = noise[..late_frames]
            .iter()
            .map(|level| level * 0.05)
            .collect();
        recorded.extend(
            envelope
                .iter()
                .zip(&noise[late_frames..])
                .map(|(level, hiss)| level * 0.4 + hiss * 0.02),
        );
        recorded
    }

    #[test]
    fn two_recordings_of_one_meeting_line_up_at_the_lag_between_them() {
        let meeting = speech(600, 7);
        // The second recorder began 42.37 seconds before the audio both share.
        let late = recorded_late(&meeting, 4_237, 99);
        assert_eq!(audio_lag(&meeting, &late), Some(-42_370));
        assert_eq!(audio_lag(&late, &meeting), Some(42_370));

        // One that started recording 95 seconds into the meeting, and stopped early.
        let joined_late: Vec<f32> = meeting[9_500..48_000]
            .iter()
            .map(|level| level * 2.0)
            .collect();
        assert_eq!(audio_lag(&meeting, &joined_late), Some(95_000));
    }

    #[test]
    fn audio_that_is_not_the_same_meeting_or_too_short_gives_no_lag() {
        let meeting = speech(600, 7);
        let another = speech(600, 12_345);
        assert_eq!(audio_lag(&meeting, &another), None);

        let half_a_minute = meeting[..3_000].to_vec();
        assert_eq!(audio_lag(&meeting, &half_a_minute), None);

        let silence = vec![0.0; 60_000];
        assert_eq!(audio_lag(&meeting, &silence), None);
    }

    fn word(text: &str, start_ms: i64) -> BuiltWord {
        BuiltWord {
            text: text.to_owned(),
            speaker: String::new(),
            start_ms,
            end_ms: start_ms + 400,
            origin: Origin::Source,
        }
    }

    /// Words of a meeting, one every 700 milliseconds from `start_ms`.
    fn said(text: &str, start_ms: i64) -> Vec<BuiltWord> {
        text.split_whitespace()
            .zip(0_i64..)
            .map(|(text, index)| word(text, start_ms + index * 700))
            .collect()
    }

    const DISCUSSION: &str = "Sharath confirmed the Elysium launch moves to Friday. Grace asked \
        whether the migration finishes before Thursday evening, and Sharath said the backups run \
        nightly anyway. Then Ada walked through the onboarding checklist, the pricing page, and \
        the customer interviews scheduled with Mooreslab next week.";

    #[test]
    fn a_transcript_alone_lines_up_by_the_words_it_shares_with_the_meetings() {
        let reference = said(DISCUSSION, 10_000);
        // The same meeting as another source heard it: its clock began 63.2 seconds later, it
        // timed its words a little differently, and it misheard some.
        let mut heard = said(
            &DISCUSSION
                .replace("Sharath", "Sheriff")
                .replace("Mooreslab", "Moore's lab"),
            10_000 - 63_200,
        );
        for (index, word) in heard.iter_mut().enumerate() {
            word.start_ms += i64::try_from(index % 3).expect("small") * 90;
        }
        let lag = words_lag(&reference, &heard).expect("a lag");
        assert!((lag - 63_200).abs() <= 200, "{lag}");
    }

    #[test]
    fn words_that_do_not_agree_on_one_lag_give_none() {
        let reference = said(DISCUSSION, 0);
        let unrelated = said(
            "Quarterly revenue planning covered hiring, offices, budgets, and travel policies \
             for every department across three regions this spring.",
            5_000,
        );
        assert_eq!(words_lag(&reference, &unrelated), None);
        // A handful of shared words are not enough to be sure.
        let few = said("Sharath confirmed Friday", 1_000);
        assert_eq!(words_lag(&reference, &few), None);
    }

    fn line(start_ms: i64, end_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
        TranscriptLine {
            start_ms,
            end_ms,
            speaker: speaker.to_owned(),
            text: text.to_owned(),
        }
    }

    fn whispered(start_ms: i64, end_ms: i64, text: &str) -> WhisperWord {
        WhisperWord {
            start_ms,
            end_ms,
            text: format!(" {text}"),
            probability: 0.9,
        }
    }

    fn times(words: &[BuiltWord]) -> Vec<(&str, i64, i64)> {
        words
            .iter()
            .map(|word| (word.text.as_str(), word.start_ms, word.end_ms))
            .collect()
    }

    #[test]
    fn a_sources_words_take_whispers_timings_where_whisper_heard_them() {
        let heard = line(1_000, 4_000, "Ada", "Sharath ships on Friday.");
        let whisper = vec![
            whispered(1_210, 1_650, "Sheriff"),
            whispered(1_700, 1_950, "ships"),
            whispered(2_000, 2_150, "on"),
            whispered(2_200, 2_700, "Friday."),
        ];
        let words = capture_words([(&heard, Origin::Source)], &whisper, 30_000);
        assert_eq!(
            times(&words),
            vec![
                // Whisper misheard the name, so it is spread from the line's start to the next
                // word Whisper anchored.
                ("Sharath", 31_000, 31_700),
                ("ships", 31_700, 31_950),
                ("on", 32_000, 32_150),
                ("Friday.", 32_200, 32_700),
            ]
        );
        assert!(words.iter().all(|word| word.speaker == "Ada"));
    }

    #[test]
    fn without_whisper_a_lines_words_spread_evenly_across_it() {
        let heard = line(0, 1_000, "Ada", "a b c");
        // Integer spreading truncates as Postgres does, so the migration's words match.
        assert_eq!(
            times(&capture_words([(&heard, Origin::Source)], &[], 0)),
            vec![("a", 0, 333), ("b", 333, 666), ("c", 666, 1_000)]
        );
    }

    #[test]
    fn whispers_words_take_the_speaker_of_a_sources_line_at_that_moment() {
        let mut words = vec![
            word("Launch", 1_000),
            word("Friday", 5_000),
            word("mumble", 20_000),
        ];
        let speaking = vec![
            Speaking {
                start_ms: 0,
                end_ms: 3_000,
                speaker: "Ada".to_owned(),
            },
            Speaking {
                start_ms: 4_000,
                end_ms: 9_000,
                speaker: "Grace".to_owned(),
            },
        ];
        name_speakers(&mut words, &speaking);
        assert_eq!(
            words
                .iter()
                .map(|word| word.speaker.as_str())
                .collect::<Vec<_>>(),
            vec!["Ada", "Grace", ""]
        );
    }

    #[test]
    fn a_moment_two_captures_heard_reads_once_and_what_only_one_heard_is_kept() {
        let teams = Uuid::now_v7();
        let fireflies = Uuid::now_v7();
        // Teams recorded from the start; Fireflies joined a moment in, heard the same words after
        // that, a little off Teams' timings, and kept recording after Teams stopped.
        let teams_words = said("Welcome everyone let us begin with the launch", 0);
        let mut fireflies_words = said("let us begin with the launch", 1_550);
        fireflies_words.extend(said("thanks all goodbye", 40_000));

        let lined = line_up(vec![(teams, teams_words), (fireflies, fireflies_words)]);
        assert_eq!(
            lined
                .iter()
                .map(|word| (word.word.text.as_str(), word.capture_id == teams))
                .collect::<Vec<_>>(),
            vec![
                ("Welcome", true),
                ("everyone", true),
                ("let", true),
                ("us", true),
                ("begin", true),
                ("with", true),
                ("the", true),
                ("launch", true),
                ("thanks", false),
                ("all", false),
                ("goodbye", false),
            ]
        );
    }

    #[test]
    fn a_later_captures_words_fill_a_gap_in_order_of_time() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        let mut first_words = said("one two", 0);
        first_words.extend(said("five six", 30_000));
        let second_words = said("three four", 12_000);
        let lined = line_up(vec![(first, first_words), (second, second_words)]);
        assert_eq!(
            lined
                .iter()
                .map(|word| word.word.text.as_str())
                .collect::<Vec<_>>(),
            vec!["one", "two", "three", "four", "five", "six"]
        );
    }
}
