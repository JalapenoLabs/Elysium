// Copyright © 2026 Jalapeno Labs

//! Fusion's rules, without a database (#94): both rankings merged by Reciprocal Rank Fusion, a
//! document ranked by its best passage, the same text twice folded into one result, and one
//! ranking alone still answering.

use chrono::{DateTime, Duration, Utc};
use uuid::Uuid;

use super::{Candidate, PASSAGES_PER_RESULT, fuse, reciprocal_rank};

/// The moment every test's documents are said at, give or take.
fn at(hours: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(1_790_000_000, 0).expect("a fixed moment") + Duration::hours(hours)
}

/// A passage of `document`, holding text that hashes to `sha`.
fn passage(document: Uuid, position: i32, sha: &str) -> Candidate {
    Candidate {
        document_id: document,
        position,
        content: format!("passage {position} of {document}"),
        source: "slack".to_owned(),
        title: format!("Document {document}"),
        url: None,
        occurred_at: at(0),
        created_by: Uuid::nil(),
        person_name: "Alex".to_owned(),
        content_sha256: sha.to_owned(),
    }
}

fn documents() -> (Uuid, Uuid, Uuid) {
    (Uuid::now_v7(), Uuid::now_v7(), Uuid::now_v7())
}

fn cited(results: &[super::SearchResult]) -> Vec<Uuid> {
    results
        .iter()
        .map(|result| result.citation.document_id)
        .collect()
}

#[test]
fn a_rank_scores_its_reciprocal_past_sixty() {
    assert!((reciprocal_rank(0) - 1.0 / 61.0).abs() < f64::EPSILON);
    assert!((reciprocal_rank(9) - 1.0 / 70.0).abs() < f64::EPSILON);
}

#[test]
fn a_passage_both_rankings_found_beats_one_found_by_either_alone() {
    let (words_only, meaning_only, both) = documents();
    let by_words = vec![passage(words_only, 0, "a"), passage(both, 0, "c")];
    let by_meaning = vec![passage(meaning_only, 0, "b"), passage(both, 0, "c")];

    let results = fuse(by_words, by_meaning, 10);

    assert_eq!(cited(&results)[0], both);
    assert_eq!(results.len(), 3);
    let expected = 2.0 * reciprocal_rank(1);
    assert!((results[0].score - expected).abs() < f64::EPSILON);
}

#[test]
fn one_ranking_alone_answers_in_its_own_order() {
    let (first, second, third) = documents();
    let by_words = vec![
        passage(first, 0, "a"),
        passage(second, 0, "b"),
        passage(third, 0, "c"),
    ];

    let results = fuse(by_words, Vec::new(), 10);

    assert_eq!(cited(&results), [first, second, third]);
}

#[test]
fn a_document_ranks_by_its_best_passage_and_shows_its_best_few() {
    let (long, short, _) = documents();
    // The long document matches five times, never first; the short one once, first.
    let mut by_words = vec![passage(short, 0, "short")];
    for position in 0..5 {
        by_words.push(passage(long, position, "long"));
    }

    let results = fuse(by_words, Vec::new(), 10);

    assert_eq!(cited(&results), [short, long]);
    let positions: Vec<i32> = results[1]
        .passages
        .iter()
        .map(|found| found.position)
        .collect();
    assert_eq!(positions.len(), PASSAGES_PER_RESULT);
    assert_eq!(positions, [0, 1, 2], "best first");
}

#[test]
fn the_same_text_twice_is_one_result_citing_the_earliest() {
    let (later, earlier, other) = documents();
    let mut later_copy = passage(later, 0, "shared");
    later_copy.occurred_at = at(5);
    later_copy.created_by = Uuid::now_v7();
    later_copy.person_name = "Jamie".to_owned();
    let mut earlier_copy = passage(earlier, 0, "shared");
    earlier_copy.occurred_at = at(1);
    let by_words = vec![
        later_copy.clone(),
        earlier_copy.clone(),
        passage(other, 0, "x"),
    ];
    let by_meaning = vec![earlier_copy, later_copy];

    let results = fuse(by_words, by_meaning, 10);

    assert_eq!(results.len(), 2, "the twins are folded");
    assert_eq!(results[0].citation.document_id, earlier);
    assert_eq!(results[0].also_recorded_by.len(), 1);
    assert_eq!(results[0].also_recorded_by[0].document_id, later);
    assert_eq!(results[0].also_recorded_by[0].person.name, "Jamie");
    assert_eq!(
        results[0].passages.len(),
        1,
        "their one passage, shown once"
    );
}

#[test]
fn the_same_text_from_another_source_stays_its_own_result() {
    let (slack, mail, _) = documents();
    let mut mailed = passage(mail, 0, "shared");
    mailed.source = "outlook".to_owned();
    let by_words = vec![passage(slack, 0, "shared"), mailed];

    let results = fuse(by_words, Vec::new(), 10);

    assert_eq!(results.len(), 2);
}

#[test]
fn no_more_than_the_limit_is_answered() {
    let by_words: Vec<Candidate> = (0..20)
        .map(|index| passage(Uuid::now_v7(), 0, &index.to_string()))
        .collect();

    assert_eq!(fuse(by_words, Vec::new(), 5).len(), 5);
}

#[test]
fn nothing_found_is_no_results() {
    assert!(fuse(Vec::new(), Vec::new(), 10).is_empty());
}
