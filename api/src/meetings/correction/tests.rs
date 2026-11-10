// Copyright © 2026 Jalapeno Labs

use chrono::Utc;
use serde_json::json;

use super::*;
use crate::company_doc::ProposalResponse;
use crate::whisper::WhisperSegment;

fn line(start_ms: i64, speaker: &str, text: &str) -> TranscriptLine {
    TranscriptLine {
        start_ms,
        end_ms: start_ms + 3_000,
        speaker: speaker.to_owned(),
        text: text.to_owned(),
    }
}

fn whisper(segments: &[(i64, &str)]) -> WhisperTranscript {
    WhisperTranscript {
        model: "Whisper large-v3".to_owned(),
        language: "en".to_owned(),
        language_probability: 0.99,
        duration_ms: 3_600_000,
        segments: segments
            .iter()
            .map(|(start_ms, text)| WhisperSegment {
                start_ms: *start_ms,
                end_ms: start_ms + 3_000,
                text: (*text).to_owned(),
                words: vec![WhisperWord {
                    start_ms: *start_ms,
                    end_ms: start_ms + 3_000,
                    text: format!(" {text}"),
                    probability: 0.9,
                }],
            })
            .collect(),
    }
}

fn doc(content: &str) -> CompanyDocResponse {
    CompanyDocResponse {
        sections: Section::ALL
            .into_iter()
            .map(|section| SectionResponse {
                section,
                purpose: section.purpose(),
                content: format!("{content} for {}", section.as_str()),
                updated_at: Utc::now(),
                proposals: Vec::new(),
            })
            .collect(),
    }
}

fn background_of(doc: &CompanyDocResponse) -> String {
    background(Background {
        title: "Weekly sync",
        participants: &["ada@example.com".to_owned()],
        transcription: "| Heard | Is |\n| --- | --- |\n| Sheriff | Sharath |",
        doc,
        speakers: &["Grace Hopper".to_owned()],
    })
}

fn fix(heard: &str, written: &str) -> Fix {
    Fix {
        heard: heard.to_owned(),
        written: written.to_owned(),
    }
}

/// Lines of a final transcript, `(speaker, text)` each, three seconds apart, a word every half
/// second, all heard by `capture`'s source.
fn words_of(capture: Uuid, lines: &[(&str, &str)]) -> Vec<Word> {
    let meeting_id = Uuid::now_v7();
    let introduced_by = Uuid::now_v7();
    let mut words = Vec::new();
    for ((speaker, text), number) in lines.iter().zip(0_i64..) {
        for (said, place) in text.split_whitespace().zip(0_i64..) {
            let start_ms = number * 10_000 + place * 500;
            words.push(Word {
                id: Uuid::now_v7(),
                meeting_id,
                position: i64::try_from(words.len() + 1).expect("small") * 1024,
                text: said.to_owned(),
                speaker: (*speaker).to_owned(),
                start_ms,
                end_ms: start_ms + 400,
                origin: Origin::Source.as_str().to_owned(),
                capture_id: Some(capture),
                introduced_by,
                superseded_by: None,
                created_at: Utc::now(),
            });
        }
    }
    words
}

/// What a capture heard, word by word, timed as [`words_of`] times the transcript.
fn version(label: &str, lines: &[&str]) -> Version {
    let words = lines
        .iter()
        .zip(0_i64..)
        .flat_map(|(text, number)| {
            text.split_whitespace()
                .zip(0_i64..)
                .map(move |(said, place)| {
                    let start_ms = number * 10_000 + place * 500;
                    BuiltWord {
                        text: said.to_owned(),
                        speaker: String::new(),
                        start_ms,
                        end_ms: start_ms + 400,
                        origin: Origin::Whisper,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect();
    Version {
        label: label.to_owned(),
        words,
    }
}

fn texts(words: &[Word], range: &Range<usize>) -> String {
    words_text(&words[range.clone()])
}

#[test]
fn a_copy_follows_the_sources_lines_or_whispers_when_the_source_sent_none() {
    let heard = whisper(&[(0, "Hello there"), (4_000, "  ")]);
    let source = vec![line(0, "Ada", "Hello there")];

    let from_source = base(source.clone(), Some(&heard)).expect("lines");
    assert_eq!(from_source.based_on, BasedOn::Source);
    assert_eq!(from_source.lines, source);

    let from_whisper = base(Vec::new(), Some(&heard)).expect("segments");
    assert_eq!(from_whisper.based_on, BasedOn::Whisper);
    assert_eq!(
        from_whisper.lines.len(),
        1,
        "a silent segment is not a line"
    );
    assert_eq!(from_whisper.lines[0].speaker, "");

    assert_eq!(base(Vec::new(), None), None);
}

#[test]
fn a_misheard_name_or_term_is_a_fix() {
    assert_eq!(
        check_fix(
            "Jon will demo Ely see um on Friday.",
            "John will demo Elysium on Friday."
        ),
        Ok(vec![fix("Jon", "John"), fix("Ely see um", "Elysium")])
    );
    // Case and the punctuation around a word change no words.
    assert_eq!(check_fix("ask jira, then", "Ask Jira then"), Ok(Vec::new()));
}

#[test]
fn a_change_that_adds_drops_or_rewrites_words_is_refused() {
    assert_eq!(
        check_fix("Launch is Friday", "Launch is Friday at noon"),
        Err(Refusal::AddsWords)
    );
    assert_eq!(
        check_fix("Launch is maybe Friday", "Launch is Friday"),
        Err(Refusal::DropsWords)
    );
    assert_eq!(
        check_fix(
            "we should talk about the budget later today",
            "we agreed the budget doubles next quarter now"
        ),
        Err(Refusal::Rewrites)
    );
    // Small replacements of more than half a run add up to a rewrite too.
    assert_eq!(
        check_fix(
            "one two three four five six seven eight nine ten",
            "uno two tres four cinco six siete ocho nueve ten"
        ),
        Err(Refusal::Rewrites)
    );
}

#[test]
fn the_transcript_reads_in_lines_of_one_speaker_broken_at_pauses() {
    let capture = Uuid::now_v7();
    let mut words = words_of(capture, &[("Ada", "We asked Sheriff"), ("Ada", "about it")]);
    // Grace answers straight after, with no pause.
    let mut answer = words_of(capture, &[("Grace", "Yes")]);
    answer[0].start_ms = 1_500;
    answer[0].end_ms = 1_800;
    words.splice(3..3, answer);

    let lines = lines_of(&words);
    let read: Vec<String> = lines.iter().map(|range| texts(&words, range)).collect();
    assert_eq!(
        read,
        vec!["We asked Sheriff", "Yes", "about it"],
        "a new speaker starts a line, and so does a pause of ten seconds"
    );

    let monologue = "word ".repeat(150);
    let long: Vec<(&str, &str)> = vec![("Ada", monologue.as_str())];
    let long_words = words_of(capture, &long);
    let long_lines = lines_of(&long_words);
    assert_eq!(
        long_lines
            .iter()
            .map(ExactSizeIterator::len)
            .collect::<Vec<_>>(),
        vec![60, 60, 30]
    );
}

#[test]
fn every_line_shows_what_each_capture_heard_otherwise() {
    let capture = Uuid::now_v7();
    let words = words_of(
        capture,
        &[
            ("Ada", "We asked Sheriff about the launch"),
            ("Grace", "Agreed."),
        ],
    );
    let transcript = Transcript::new(
        &words,
        vec![
            version(
                "Fireflies",
                &["We asked Sheriff about the launch", "Agreed."],
            ),
            version("Whisper", &["We asked Sharath about the launch", "agreed"]),
        ],
    );

    let text = prompt(&background_of(&doc("- Entry")), &transcript);

    assert!(text.contains(
        "[0] [0:00] Ada: We asked Sheriff about the launch\n    Whisper: We asked Sharath about \
         the launch\n[1] [0:10] Grace: Agreed.\n"
    ));
    assert!(
        !text.contains("    Fireflies:"),
        "a capture that heard it the same way is not repeated"
    );
    assert!(
        !text.contains("    Whisper: agreed"),
        "nor one differing in case alone"
    );
    let doc_at = text
        .find("The Transcription doc")
        .expect("the Transcription doc");
    let lines_at = text.find("[0] [0:00]").expect("the lines");
    assert!(doc_at < lines_at, "the doc is read before the transcript");
    assert!(text.contains("| Sheriff | Sharath |"));
    assert!(text.contains("Grace Hopper"));
}

#[test]
fn a_long_meeting_is_one_prompt_with_every_line() {
    let capture = Uuid::now_v7();
    // Thirteen hours of talk with a pause after every sentence, several times what one argument
    // could carry.
    let said = format!("line {}", "word ".repeat(15));
    let lines: Vec<(&str, &str)> = (0..4_800).map(|_index| ("Ada", said.as_str())).collect();
    let words = words_of(capture, &lines);
    let transcript = Transcript::new(&words, Vec::new());
    let background = background_of(&doc("- Ada Lovelace"));

    let text = prompt(&background, &transcript);

    assert!(text.len() > 3 * 128 * 1024, "{} bytes", text.len());
    assert_eq!(text.matches("## People").count(), 1, "the doc is read once");
    assert_eq!(
        text.matches("] Ada: line").count(),
        4_800,
        "every line is whole"
    );
}

#[test]
fn the_doc_spells_people_and_terms_first_and_marks_what_is_only_proposed() {
    let mut current = doc("- Entry");
    let glossary = current
        .sections
        .iter_mut()
        .find(|section| section.section == Section::Glossary)
        .expect("a glossary");
    glossary.proposals.push(ProposalResponse {
        id: Uuid::now_v7(),
        section: "glossary".to_owned(),
        base_content: "- Entry for glossary".to_owned(),
        content: "- Elysium\n- Entry for glossary".to_owned(),
        reason: "Heard in a meeting".to_owned(),
        quote: None,
        source: None,
        timeline_record_id: None,
        workflow_run_id: None,
        state: "pending".to_owned(),
        decided_by: None,
        decided_at: None,
        created_by: Uuid::now_v7(),
        agent: None,
        created_at: Utc::now(),
    });

    let shared = background_of(&current);

    let people = shared.find("## People").expect("people");
    let glossary = shared.find("## Glossary").expect("glossary");
    let company = shared.find("## Company").expect("company");
    assert!(people < glossary && glossary < company);
    assert!(shared.contains("(proposed, waiting for review) - Elysium"));
    assert!(!shared.contains("(proposed, waiting for review) - Entry"));
}

/// A meeting of two lines, the second with a name heard as three words, beside Whisper's.
fn two_lines() -> (Vec<Word>, Vec<Version>) {
    let words = words_of(
        Uuid::now_v7(),
        &[
            ("Ada", "We asked Sheriff about the launch"),
            ("Grace", "Ely see um ships Friday"),
        ],
    );
    let heard = vec![version(
        "Whisper",
        &["We asked Sharath about the lunch", "Elysium ships Friday"],
    )];
    (words, heard)
}

#[test]
fn a_flag_covers_the_words_it_copies_from_its_line() {
    let (words, heard) = two_lines();
    let transcript = Transcript::new(&words, heard);
    let answer = r#"Here: {"flags": [
        {"line": 0, "heard": "sheriff", "text": "Sharath", "confidence": 92,
         "alternatives": ["Sharad", "Sharath", "Sharad", "  ", "Sharat the third and fourth"],
         "why": "The company doc's People   spell him Sharath."},
        {"line": 1, "heard": "Ely see um", "text": "Elysium", "confidence": 60},
        {"line": 1, "heard": "Friday", "text": "Friday", "confidence": 0}
    ]}"#;

    let checked = parse_answer(answer, &transcript).expect("parses");

    assert_eq!(checked.refused, Vec::new());
    let [sharath, elysium, unsure] = checked.flags.as_slice() else {
        panic!("three flags, not {:?}", checked.flags);
    };
    assert_eq!(texts(&words, &sharath.words), "Sheriff");
    assert_eq!(sharath.text, "Sharath");
    assert_eq!(sharath.confidence, 92);
    assert_eq!(
        sharath.alternatives,
        vec!["Sharad"],
        "each once, never the text itself, blank, or a rewrite"
    );
    assert_eq!(
        sharath.why.as_deref(),
        Some("The company doc's People spell him Sharath.")
    );
    assert_eq!(sharath.fixes, vec![fix("Sheriff", "Sharath")]);
    assert!(sharath.changes_text);
    assert_eq!(
        sharath.mark(),
        json!({ "confidence": 92, "alternatives": ["Sharad"] })
    );

    assert_eq!(texts(&words, &elysium.words), "Ely see um");
    assert_eq!(elysium.fixes, vec![fix("Ely see um", "Elysium")]);

    // Unsure what was said, it leaves the word as it is, for a person to listen to.
    assert_eq!(texts(&words, &unsure.words), "Friday");
    assert_eq!((unsure.confidence, unsure.changes_text), (0, false));
    assert_eq!(
        unsure.mark(),
        json!({ "confidence": 0, "alternatives": [] })
    );
}

#[test]
fn a_flag_is_kept_only_over_words_it_was_given_without_overlap() {
    let (words, heard) = two_lines();
    let transcript = Transcript::new(&words, heard);
    let answer = r#"{"flags": [
        {"line": 9, "heard": "Sheriff", "text": "Sharath", "confidence": 90},
        {"line": "zero", "heard": "Sheriff", "text": "Sharath", "confidence": 90},
        {"line": 0, "heard": "Shereef", "text": "Sharath", "confidence": 90},
        {"line": 0, "heard": "Sheriff", "text": " ", "confidence": 90},
        {"line": 0, "heard": "Sheriff about", "text": "Sharath about it", "confidence": 90},
        {"line": 0, "heard": "We asked Sheriff about the launch",
         "text": "They told nobody anything about lunch", "confidence": 90},
        {"line": 0, "heard": "Sheriff", "text": "Sharath", "confidence": 150},
        {"line": 0, "heard": "Sheriff about", "text": "Sharath about", "confidence": 90},
        {"line": 1, "heard": "ships", "text": "slips", "confidence": -4},
        {"line": 1, "heard": "Friday", "text": "Friday"},
        {"line": 1, "heard": "Friday", "text": "Fryday"}
    ]}"#;

    let checked = parse_answer(answer, &transcript).expect("parses");

    let refusals: Vec<Refusal> = checked
        .refused
        .iter()
        .map(|(_, refusal)| *refusal)
        .collect();
    assert_eq!(
        refusals,
        vec![
            Refusal::UnknownLine,
            Refusal::UnknownLine,
            Refusal::NotInLine,
            Refusal::Empty,
            Refusal::AddsWords,
            Refusal::Rewrites,
            Refusal::Overlaps,
        ]
    );
    let kept: Vec<(String, &str, u8)> = checked
        .flags
        .iter()
        .map(|flag| {
            (
                texts(&words, &flag.words),
                flag.text.as_str(),
                flag.confidence,
            )
        })
        .collect();
    assert_eq!(
        kept,
        vec![
            ("Sheriff".to_owned(), "Sharath", 100),
            ("ships".to_owned(), "slips", 0),
            // A change with no confidence is held at the middle of the scale; a flag that
            // changes nothing and says nothing is dropped.
            ("Friday".to_owned(), "Fryday", CHANGED_CONFIDENCE),
        ],
        "confidences clamped to 0 to 100"
    );

    let doubled = words_of(Uuid::now_v7(), &[("Ada", "the plan and the launch")]);
    let ambiguous = Transcript::new(&doubled, Vec::new());
    let checked = parse_answer(
        r#"{"flags": [{"line": 0, "heard": "the", "text": "a", "confidence": 90}]}"#,
        &ambiguous,
    )
    .expect("parses");
    assert_eq!(checked.refused, vec![("0".to_owned(), Refusal::Ambiguous)]);

    assert_eq!(parse_answer("no", &transcript), Err(AnswerError::NoJson));
    assert!(matches!(
        parse_answer("{\"edits\": []}", &transcript),
        Err(AnswerError::Shape(_))
    ));
}

#[test]
fn what_a_capture_heard_otherwise_with_no_flag_is_marked_changed_with_what_it_heard() {
    let (words, heard) = two_lines();
    let mut versions = heard;
    // A second capture heard the launch as a lunch too, and the name another way.
    versions.push(version(
        "Capture 2, Teams",
        &["We asked Shareef about the lunch", "Elysium ships Friday"],
    ));
    let transcript = Transcript::new(&words, versions);
    let checked = parse_answer(
        r#"{"flags": [{"line": 1, "heard": "Ely see um", "text": "Elysium", "confidence": 95}]}"#,
        &transcript,
    )
    .expect("parses");

    let changed = unflagged_changes(&transcript, &checked.flags);

    let read: Vec<(String, Vec<String>)> = changed
        .iter()
        .map(|run| (texts(&words, &run.words), run.alternatives.clone()))
        .collect();
    assert_eq!(
        read,
        vec![
            (
                "Sheriff".to_owned(),
                vec!["Sharath".to_owned(), "Shareef".to_owned()]
            ),
            ("launch".to_owned(), vec!["lunch".to_owned()]),
        ],
        "what the flag covers is the flag's; each capture's reading is an alternative once"
    );
    assert_eq!(
        changed[1].mark(),
        json!({ "confidence": 50, "alternatives": ["lunch"] })
    );

    // A person's edit is theirs, and is never marked changed.
    let mut edited = words.clone();
    edited[2].origin = Origin::Edit.as_str().to_owned();
    let transcript = Transcript::new(&edited, two_lines().1);
    let changed = unflagged_changes(&transcript, &[]);
    assert!(
        changed
            .iter()
            .all(|run| texts(&edited, &run.words) != "Sheriff"),
        "{changed:?}"
    );
}

#[test]
fn runs_two_captures_heard_otherwise_that_overlap_are_one_mark() {
    let words = words_of(Uuid::now_v7(), &[("Ada", "ask Moore lab now")]);
    let transcript = Transcript::new(
        &words,
        vec![
            version("Fireflies", &["ask More lab now"]),
            version("Whisper", &["ask Mooreslab now"]),
        ],
    );
    let changed = unflagged_changes(&transcript, &[]);
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert_eq!(texts(&words, &changed[0].words), "Moore lab");
    assert_eq!(changed[0].alternatives, vec!["More lab", "Mooreslab"]);
}

#[test]
fn the_corrected_copy_takes_each_flag_inside_one_of_the_first_captures_lines() {
    let capture = Uuid::now_v7();
    let base = Base {
        based_on: BasedOn::Source,
        lines: vec![
            TranscriptLine {
                start_ms: 0,
                end_ms: 3_000,
                speaker: "Ada".to_owned(),
                text: "We asked Sheriff about it".to_owned(),
            },
            TranscriptLine {
                start_ms: 3_000,
                end_ms: 6_000,
                speaker: "Grace".to_owned(),
                text: "Ely see um ships".to_owned(),
            },
        ],
    };
    // The final transcript as it was built from those lines, with a word of another capture.
    let built =
        alignment::capture_words(base.lines.iter().map(|line| (line, Origin::Source)), &[], 0);
    let introduced_by = Uuid::now_v7();
    let mut words: Vec<Word> = built
        .into_iter()
        .zip(1_i64..)
        .map(|(word, number)| Word {
            id: Uuid::now_v7(),
            meeting_id: Uuid::now_v7(),
            position: number * 1024,
            text: word.text,
            speaker: word.speaker,
            start_ms: word.start_ms,
            end_ms: word.end_ms,
            origin: Origin::Source.as_str().to_owned(),
            capture_id: Some(capture),
            introduced_by,
            superseded_by: None,
            created_at: Utc::now(),
        })
        .collect();
    let mut elsewhere = words[0].clone();
    elsewhere.id = Uuid::now_v7();
    elsewhere.capture_id = Some(Uuid::now_v7());
    elsewhere.text = "Hi".to_owned();
    elsewhere.start_ms = 9_000;
    elsewhere.end_ms = 9_500;
    words.push(elsewhere);

    let places = places_in_lines(&base, &[], 0, capture, &words);
    assert_eq!(places.len(), 9, "every word of the capture's own lines");
    let transcript = Transcript::new(&words, Vec::new());
    let checked = parse_answer(
        r#"{"flags": [
            {"line": 0, "heard": "Sheriff", "text": "Sharath", "confidence": 90},
            {"line": 1, "heard": "Ely see um", "text": "Elysium", "confidence": 90},
            {"line": 2, "heard": "Hi", "text": "Hey", "confidence": 90}
        ]}"#,
        &transcript,
    )
    .expect("parses");
    assert_eq!(checked.flags.len(), 3, "{checked:?}");

    let (copy, changed) = corrected_copy(&base, &places, &transcript, &checked.flags);

    assert_eq!(changed, 2);
    assert_eq!(copy[0].text, "We asked Sharath about it");
    assert_eq!(copy[1].text, "Elysium ships");
    for (original, copied) in base.lines.iter().zip(&copy) {
        assert_eq!(
            (original.start_ms, original.end_ms, &original.speaker),
            (copied.start_ms, copied.end_ms, &copied.speaker),
            "only the text changes"
        );
    }
    assert_eq!(
        distinct_fixes(&checked.flags),
        vec![
            fix("Sheriff", "Sharath"),
            fix("Ely see um", "Elysium"),
            fix("Hi", "Hey")
        ]
    );
    assert_ne!(transcript_hash(&copy), transcript_hash(&base.lines));
}
