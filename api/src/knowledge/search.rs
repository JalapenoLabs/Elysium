// Copyright © 2026 Jalapeno Labs

//! Hybrid search of the knowledge index: its words and its meaning, ranked together.
//!
//! [`KnowledgeSearch::search`] is the one search of the index. The Knowledge page and the global
//! search ask it through `GET /api/v1/knowledge/search`, and the MCP server's `knowledge_search`
//! calls it too, so every caller finds the same passages in the same order. See
//! `docs/knowledge.md`, Search.
//!
//! # Two rankings
//!
//! - **Words.** The query is parsed with `websearch_to_tsquery('english', ..)`, the parser and
//!   stemmer the passages' generated `search_vector` uses, so both sides cut text alike: a key such
//!   as `WAY-123` is the phrase `'way' <-> '-123'` on both, and a question loses its stop words.
//!   It never fails on what was typed, and reads quotes as phrases, `or` as either, and a leading
//!   `-` as without, as a search box does. Matches rank by `ts_rank_cd`, which favours passages
//!   whose matched words sit close together.
//! - **Meaning.** The query is embedded by the embedding model (`crate::embeddings`) and the
//!   nearest passages by cosine distance are read through the HNSW index, as far as
//!   [`MAX_MEANING_DISTANCE`]: the nearest passages are always some distance away, and past it
//!   they are unrelated, so a query about nothing indexed answers nothing rather than whatever
//!   happens to be nearest.
//!
//! Each takes its closest [`CANDIDATES`] passages, under the same filters.
//!
//! # Fusion
//!
//! The two lists are merged by Reciprocal Rank Fusion (Cormack, Clarke, and Büttcher, SIGIR
//! 2009): a passage scores `1 / (60 + rank)` in each list it is in, summed. It reads ranks
//! alone, so `ts_rank_cd`'s unbounded numbers and cosine distances never need weighing against
//! each other, and a passage found by one ranking alone (not embedded yet, or sharing no word
//! with the query) still scores from that one. It is the method Elasticsearch and Azure AI
//! Search ship for merging lexical and vector results, and 60 is the paper's constant, which it
//! found robust across collections.
//!
//! A document ranks by its best passage, so a long document is not favoured for matching often,
//! and shows up to [`PASSAGES_PER_RESULT`] of its passages, best first.
//!
//! # The same text twice
//!
//! Two people whose connectors read the same channel or space each have the record, so the
//! index holds two documents of the same source with the same `content_sha256`. Their passages
//! are the same text at the same positions, so they rank alike, and a result folds them into
//! one: the earliest recorded is cited, and the others are listed as also recorded by their
//! people. Pages are not folded: two pages saying the same thing are two pages.
//!
//! # When meaning is not available
//!
//! A query the embedding model cannot embed in time (the service is starting, stopped, or busy)
//! is searched by its words alone, and the answer says so. A passage not embedded yet is found
//! by its words alone. Nothing about the embedding model ever fails a search. The service going
//! away is a warning once and coming back an info line, as the embedder logs it; the log carries
//! counts and durations, never the query or a passage.
//!
//! # Budget
//!
//! Each person may search [`PER_PERSON`]'s rate, however they search: [`KnowledgeSearch::admit`]
//! spends it, and every caller asks it first.
//!
//! # Who sees what
//!
//! Nothing here limits results by whose record a passage came from: every member reads
//! everything indexed (`docs/knowledge.md`, Who sees what), but for a private meeting's segments,
//! which only its readers find, and what agents wrote from a meeting, which only those who may read
//! every meeting it names find. Each search finds a meeting's segments only for a reader who may
//! read the meeting (`models::meeting::readable_meeting_sql`), and a record only for a reader who
//! may read every meeting it names (`models::meeting_reads::readable_record_sql`), in both
//! rankings, so a meeting made private is gone from the next search at once. Callers check the
//! caller's role; the person filter only narrows.

use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::time::Instant;

use anyhow::Context;
use chrono::{DateTime, Utc};
use diesel::QueryableByName;
use diesel::sql_types::{
    Array, BigInt, Double, Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid,
};
use diesel_async::{AsyncConnection, RunQueryDsl};
use serde::Serialize;
use tracing::{Level, event};
use uuid::Uuid;

use crate::database::Pool;
use crate::embeddings::{Embedding, Embeddings, MODEL};
use crate::middleware::rate_limit::{Decision, Limiter, Rate};
use crate::models::knowledge_document::{Citation, Copy, Passage, Person};
use crate::models::meeting::{Reader, readable_meeting_sql};
use crate::models::meeting_reads::readable_record_sql;
use crate::models::{page_suggestion, user};

/// The longest query searched, in characters: a search box's few words, or an agent's
/// question in a sentence or two.
pub const MAX_QUERY_CHARS: u64 = 500;

/// Results when the caller names no limit: a page of results on the Knowledge page.
pub const DEFAULT_LIMIT: usize = 10;

/// The most results one search answers.
pub const MAX_LIMIT: usize = 50;

/// The most sources one search filters by: every kind the index holds, with room to spare.
pub const MAX_SOURCES: u64 = 32;

/// How many passages each ranking contributes before they are fused: enough that a result's
/// best passages, and a document's twins, are among them for [`MAX_LIMIT`] results, while both
/// queries stay a few milliseconds. The HNSW scan's `ef_search` is raised to match, since it
/// answers at most that many rows.
const CANDIDATES: i64 = 100;

/// The farthest, by cosine distance, a passage may be from the query to be found by meaning:
/// a cosine similarity of 0.55. Measured with nomic-embed-text-v1.5 on realistic mail, chat,
/// pages, meeting segments, and learnings (#94), passages about what a query asked lay 0.23 to
/// 0.40 from it, and unrelated ones 0.42 to 0.53, so this keeps every related passage with room
/// to spare and drops most of the unrelated. A passage past it can still be found by its words.
/// It belongs to the model: another model is measured again.
const MAX_MEANING_DISTANCE: f64 = 0.45;

/// Reciprocal Rank Fusion's constant (Fusion, above): a rank of `r` scores `1 / (60 + r)`.
/// Larger evens out the top ranks; 60 is the value the method was published and validated with.
const RRF_K: f64 = 60.0;

/// The most passages a result shows: its best, so a long meeting segment cites the lines that
/// matched rather than all of them.
pub const PASSAGES_PER_RESULT: usize = 3;

/// One person's searches of the knowledge base. Each embeds its query on the service the
/// engine's embedder also uses, so it is held lower than the global search's titles: twenty at
/// once and two a second covers the topbar and the Knowledge page asking after every pause in
/// typing, while a script looping over the route cannot crowd out the embedder.
pub const PER_PERSON: Rate = Rate {
    per_second: 2,
    burst: 20,
};

/// What a search narrows to. Every filter is a narrowing the caller asks for, never access
/// control (`docs/knowledge.md`, Who sees what).
#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    /// Only documents of these sources (`knowledge_documents.source`), or every source when
    /// empty.
    pub sources: Vec<String>,
    /// Only timeline records attributed to this project and pages linked to it. Whatever else is
    /// indexed belongs to no project, so it is left out.
    pub project_id: Option<Uuid>,
    /// Only documents from this person's records, learnings, or edits.
    pub person_id: Option<Uuid>,
    /// Only what was said at or after this moment.
    pub occurred_from: Option<DateTime<Utc>>,
    /// Only what was said before this moment.
    pub occurred_before: Option<DateTime<Utc>>,
    /// Only pages carrying this label, in the form `page_labels` stores it. Nothing else has
    /// labels, so everything else is left out.
    pub label: Option<String>,
}

/// One search: whose it is, what was typed or asked, what it narrows to, and how many results it
/// answers.
#[derive(Debug, Clone)]
pub struct SearchRequest {
    /// Who the search is for, by user id: a member, or the machine an agent's turn reads as. The
    /// segments of the meetings they may not read are left out.
    pub reader: Uuid,
    pub query: String,
    pub filters: SearchFilters,
    /// From 1 to [`MAX_LIMIT`].
    pub limit: usize,
}

/// One document found, with its best passages.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub citation: Citation,
    /// Up to [`PASSAGES_PER_RESULT`], best first.
    pub passages: Vec<Passage>,
    /// Others whose records hold the same text, earliest first.
    pub also_recorded_by: Vec<Copy>,
    /// Its fused score: higher ranks first. Comparable within one answer only.
    pub score: f64,
    /// How many edits agents suggested to the page it was indexed from wait for a person, so a
    /// result says so; 0 for anything that is no page.
    pub pending_suggestions: i64,
}

/// What a search found, best first.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchAnswer {
    pub results: Vec<SearchResult>,
    /// Whether the query was searched by meaning as well as by words. False while the
    /// embedding model cannot embed it.
    pub searched_by_meaning: bool,
}

/// Searches the knowledge index. Cheap to clone; clones share each person's budget and whether
/// meaning is available.
#[derive(Clone)]
pub struct KnowledgeSearch {
    embeddings: Embeddings,
    /// Each person's searches, keyed by their user id. Signed-in people, whom the `users` table
    /// bounds, so nothing sweeps them.
    budget: Arc<Limiter<Uuid>>,
    /// Whether the last query could not be embedded, so the service going away is logged once.
    meaning_unavailable: Arc<AtomicBool>,
}

impl std::fmt::Debug for KnowledgeSearch {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KnowledgeSearch")
            .field("embeddings", &self.embeddings)
            .field("meaning_unavailable", &self.meaning_unavailable)
            .finish_non_exhaustive()
    }
}

/// A passage one ranking found, with what its result cites.
#[derive(Debug, Clone, QueryableByName)]
struct Candidate {
    #[diesel(sql_type = SqlUuid)]
    document_id: Uuid,
    #[diesel(sql_type = Integer)]
    position: i32,
    #[diesel(sql_type = Text)]
    content: String,
    #[diesel(sql_type = Text)]
    source: String,
    #[diesel(sql_type = Text)]
    title: String,
    #[diesel(sql_type = Nullable<Text>)]
    url: Option<String>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    page_id: Option<Uuid>,
    #[diesel(sql_type = Nullable<SqlUuid>)]
    meeting_id: Option<Uuid>,
    #[diesel(sql_type = Timestamptz)]
    occurred_at: DateTime<Utc>,
    #[diesel(sql_type = SqlUuid)]
    created_by: Uuid,
    #[diesel(sql_type = Text)]
    person_name: String,
    #[diesel(sql_type = Text)]
    content_sha256: String,
}

impl Candidate {
    fn citation(&self) -> Citation {
        Citation {
            document_id: self.document_id,
            source: self.source.clone(),
            title: self.title.clone(),
            url: self.url.clone(),
            page_id: self.page_id,
            meeting_id: self.meeting_id,
            occurred_at: self.occurred_at,
            person: Person {
                id: self.created_by,
                name: self.person_name.clone(),
            },
        }
    }
}

/// The filters both rankings apply, binding `$2` to `$7` in [`SearchFilters`]' order, and `$8`
/// and `$9`, the reader and their reach, whom a meeting's segments, and records written from
/// meetings, are found by only if they may read every meeting they name. A macro, so each query
/// stays one string literal.
macro_rules! filters {
    () => {
        concat!(
            "AND (cardinality($2::text[]) = 0 OR documents.source = ANY($2::text[]))
         AND ($3::uuid IS NULL
             OR documents.timeline_record_id IN (
                 SELECT timeline_record_id FROM timeline_record_projects WHERE project_id = $3::uuid
             )
             OR documents.page_id IN (
                 SELECT page_id FROM page_projects WHERE project_id = $3::uuid
             ))
         AND ($4::uuid IS NULL OR documents.created_by = $4::uuid)
         AND ($5::timestamptz IS NULL OR documents.occurred_at >= $5::timestamptz)
         AND ($6::timestamptz IS NULL OR documents.occurred_at < $6::timestamptz)
         AND ($7::text IS NULL OR documents.page_id IN (
             SELECT page_id FROM page_labels WHERE label = $7::text
         ))
         AND (documents.access_meeting_id IS NULL OR ",
            readable_meeting_sql!("documents.access_meeting_id", "$8", "$9"),
            ") AND ",
            readable_record_sql!("documents.timeline_record_id", "$8", "$9")
        )
    };
}

/// What a candidate carries besides its ranking, from its passage, document, and person.
macro_rules! candidate_columns {
    () => {
        "passages.document_id, passages.position, passages.content, documents.source,
         documents.title, documents.url, documents.page_id, documents.meeting_id,
         documents.occurred_at,
         documents.created_by, people.name AS person_name, documents.content_sha256"
    };
}

/// The passages whose words match `$1`, closest first. Ties read newest document first, then
/// in the document's order, so an answer is the same every time.
const BY_WORDS: &str = concat!(
    "SELECT ",
    candidate_columns!(),
    " FROM knowledge_passages passages
     JOIN knowledge_documents documents ON documents.id = passages.document_id
     JOIN users people ON people.id = documents.created_by
     CROSS JOIN websearch_to_tsquery('english', $1) query
     WHERE passages.search_vector @@ query ",
    filters!(),
    " ORDER BY ts_rank_cd(passages.search_vector, query) DESC,
         passages.document_id DESC, passages.position
     LIMIT $10"
);

/// The passages nearest `$1`, an embedding in pgvector's text form, by cosine distance through
/// the HNSW index, among those embedded by `$11`. The scan is iterative (`hnsw.iterative_scan`),
/// so filters that leave out most neighbours still answer the full limit; its order is then
/// relaxed, so the outer query orders again. Those farther than `$12` are left out after the
/// scan, never during it, which would make it read on in search of more.
const BY_MEANING: &str = concat!(
    "WITH nearest AS MATERIALIZED (
         SELECT passages.document_id, passages.position,
             passages.embedding <=> $1::vector AS distance
         FROM knowledge_passages passages
         JOIN knowledge_documents documents ON documents.id = passages.document_id
         WHERE passages.embedding_model = $11 ",
    filters!(),
    " ORDER BY distance
         LIMIT $10
     )
     SELECT ",
    candidate_columns!(),
    " FROM nearest
     JOIN knowledge_passages passages
         ON passages.document_id = nearest.document_id AND passages.position = nearest.position
     JOIN knowledge_documents documents ON documents.id = passages.document_id
     JOIN users people ON people.id = documents.created_by
     WHERE nearest.distance <= $12
     ORDER BY nearest.distance, passages.document_id DESC, passages.position"
);

impl KnowledgeSearch {
    pub fn new(embeddings: Embeddings) -> Self {
        Self {
            embeddings,
            budget: Arc::new(Limiter::new(PER_PERSON)),
            meaning_unavailable: Arc::new(AtomicBool::new(false)),
        }
    }

    /// This search with every person's budget started afresh and timed by `clock`, so a test
    /// can hold time still while it counts a budget out.
    #[cfg(test)]
    pub fn with_budget_clock(self, clock: crate::middleware::rate_limit::LimiterClock) -> Self {
        Self {
            budget: Arc::new(Limiter::with_clock(clock, PER_PERSON)),
            ..self
        }
    }

    /// Spends one of `person`'s searches, or answers how long until they have one again.
    /// Every caller asks before it searches, so a person's budget is one however they search.
    pub fn admit(&self, person: Uuid) -> Decision {
        self.budget.decide(&person)
    }

    /// Searches the index for `request`, by words and by meaning, and answers the closest
    /// documents with their best passages (see the module's documentation).
    ///
    /// A query with nothing in it but spaces answers no results without searching.
    ///
    /// # Errors
    /// A database that does not answer. The embedding model never fails a search: without it
    /// the query is searched by its words alone.
    pub async fn search(
        &self,
        database: &Pool,
        request: &SearchRequest,
    ) -> anyhow::Result<SearchAnswer> {
        let query = request.query.trim();
        if query.is_empty() {
            event!(
                name: "knowledge.search.empty",
                Level::DEBUG,
                "a knowledge search held nothing to search for",
            );
            return Ok(SearchAnswer {
                results: Vec::new(),
                searched_by_meaning: false,
            });
        }

        let started = Instant::now();
        // Before a connection is taken, so none is held while the model works.
        let embedding = self.embed(query).await;
        let embedded_ms = elapsed_ms(started);

        let mut connection = database
            .get()
            .await
            .context("no database connection available")?;
        let filters = &request.filters;
        let (by_words, by_meaning) = connection
            .transaction(async |connection| {
                let reader = Reader::of(&user::find(connection, request.reader).await?);
                let by_words = diesel::sql_query(BY_WORDS)
                    .bind::<Text, _>(query)
                    .bind::<Array<Text>, _>(&filters.sources)
                    .bind::<Nullable<SqlUuid>, _>(filters.project_id)
                    .bind::<Nullable<SqlUuid>, _>(filters.person_id)
                    .bind::<Nullable<Timestamptz>, _>(filters.occurred_from)
                    .bind::<Nullable<Timestamptz>, _>(filters.occurred_before)
                    .bind::<Nullable<Text>, _>(&filters.label)
                    .bind::<SqlUuid, _>(reader.id)
                    .bind::<Text, _>(reader.reach.as_str())
                    .bind::<BigInt, _>(CANDIDATES)
                    .load::<Candidate>(connection)
                    .await?;

                let Some(embedding) = &embedding else {
                    return Ok::<_, diesel::result::Error>((by_words, Vec::new()));
                };
                // The scan answers at most `ef_search` rows, 40 by default; and without the
                // iterative scan, a filter leaving out most of those would answer fewer than
                // asked. Both last until the transaction ends.
                diesel::sql_query(format!("SET LOCAL hnsw.ef_search = {CANDIDATES}"))
                    .execute(connection)
                    .await?;
                diesel::sql_query("SET LOCAL hnsw.iterative_scan = relaxed_order")
                    .execute(connection)
                    .await?;
                let by_meaning = diesel::sql_query(BY_MEANING)
                    .bind::<Text, _>(embedding.to_pgvector())
                    .bind::<Array<Text>, _>(&filters.sources)
                    .bind::<Nullable<SqlUuid>, _>(filters.project_id)
                    .bind::<Nullable<SqlUuid>, _>(filters.person_id)
                    .bind::<Nullable<Timestamptz>, _>(filters.occurred_from)
                    .bind::<Nullable<Timestamptz>, _>(filters.occurred_before)
                    .bind::<Nullable<Text>, _>(&filters.label)
                    .bind::<SqlUuid, _>(reader.id)
                    .bind::<Text, _>(reader.reach.as_str())
                    .bind::<BigInt, _>(CANDIDATES)
                    .bind::<Text, _>(MODEL)
                    .bind::<Double, _>(MAX_MEANING_DISTANCE)
                    .load::<Candidate>(connection)
                    .await?;
                Ok((by_words, by_meaning))
            })
            .await
            .context("cannot search the knowledge index")?;

        let found_by_words = by_words.len();
        let found_by_meaning = by_meaning.len();
        let mut results = fuse(by_words, by_meaning, request.limit);
        let page_ids: Vec<Uuid> = results
            .iter()
            .filter_map(|result| result.citation.page_id)
            .collect();
        let waiting = page_suggestion::pending_counts(&mut connection, Some(&page_ids))
            .await
            .context("cannot count the suggestions waiting on the pages found")?;
        drop(connection);
        for result in &mut results {
            let page_id = result.citation.page_id;
            result.pending_suggestions = page_id
                .and_then(|id| waiting.get(&id).copied())
                .unwrap_or_default();
        }
        event!(
            name: "knowledge.search.answered",
            Level::DEBUG,
            search.by_words = found_by_words,
            search.by_meaning = found_by_meaning,
            search.results = results.len(),
            search.embedded_ms = embedded_ms,
            search.duration_ms = elapsed_ms(started),
            "a knowledge search answered {{search.results}} results in {{search.duration_ms}} ms",
        );
        Ok(SearchAnswer {
            results,
            searched_by_meaning: embedding.is_some(),
        })
    }

    /// The query's embedding, or none when the embedding model cannot give it now. Logged once
    /// when the model goes away and once when it comes back.
    async fn embed(&self, query: &str) -> Option<Embedding> {
        match self.embeddings.embed_query(query).await {
            Ok(embedding) => {
                if self
                    .meaning_unavailable
                    .swap(false, atomic::Ordering::Relaxed)
                {
                    event!(
                        name: "knowledge.search.meaning_restored",
                        Level::INFO,
                        "knowledge searches are searched by meaning again",
                    );
                }
                Some(embedding)
            }
            Err(error) => {
                let already = self
                    .meaning_unavailable
                    .swap(true, atomic::Ordering::Relaxed);
                if already {
                    event!(
                        name: "knowledge.search.meaning_still_unavailable",
                        Level::DEBUG,
                        error.message = %error,
                        "a knowledge search was searched by its words alone",
                    );
                } else {
                    event!(
                        name: "knowledge.search.meaning_unavailable",
                        Level::WARN,
                        error.message = %error,
                        "knowledge searches are searched by their words alone until the \
                         embedding model answers",
                    );
                }
                None
            }
        }
    }
}

fn elapsed_ms(started: Instant) -> u64 {
    u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// What Reciprocal Rank Fusion gives the passage at `index` of a ranking, from 0.
fn reciprocal_rank(index: usize) -> f64 {
    // A ranking holds at most `CANDIDATES` passages, which an f64 holds exactly.
    #[expect(
        clippy::cast_precision_loss,
        reason = "an index below a few hundred is exact as f64"
    )]
    let rank = (index + 1) as f64;
    1.0 / (RRF_K + rank)
}

/// One folded result while it is gathered: the documents holding its text, and its passages
/// by position with their scores.
#[derive(Default)]
struct Gathered {
    score: f64,
    documents: Vec<Citation>,
    passages: HashMap<i32, (f64, String)>,
}

/// Merges the two rankings into at most `limit` results, best first (Fusion and The same text
/// twice, above). Pure, so its rules are tested without a database.
fn fuse(by_words: Vec<Candidate>, by_meaning: Vec<Candidate>, limit: usize) -> Vec<SearchResult> {
    // Every passage's summed score, by document and position.
    let mut scored: HashMap<(Uuid, i32), (f64, Candidate)> = HashMap::new();
    for ranking in [by_words, by_meaning] {
        for (index, candidate) in ranking.into_iter().enumerate() {
            let key = (candidate.document_id, candidate.position);
            scored.entry(key).or_insert((0.0, candidate)).0 += reciprocal_rank(index);
        }
    }

    // Folded by what the text is: documents of one source holding the same text are one result.
    // A page is never folded: two pages saying the same thing are two pages, each with its own
    // place in the tree, where two people's records of one message are one thing said.
    let mut gathered: HashMap<(String, String, Option<Uuid>), Gathered> = HashMap::new();
    for (score, candidate) in scored.into_values() {
        // A meeting's whole transcript is its own place too, the meeting's page.
        let key = (
            candidate.source.clone(),
            candidate.content_sha256.clone(),
            candidate.page_id.or(candidate.meeting_id),
        );
        let result = gathered.entry(key).or_default();
        result.score = result.score.max(score);
        if !result
            .documents
            .iter()
            .any(|document| document.document_id == candidate.document_id)
        {
            result.documents.push(candidate.citation());
        }
        let passage = result
            .passages
            .entry(candidate.position)
            .or_insert((score, candidate.content));
        // Twins hold the same text at the same place; whichever ranked it higher counts.
        passage.0 = passage.0.max(score);
    }

    let mut results: Vec<SearchResult> = gathered.into_values().map(into_result).collect();
    results.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(Ordering::Equal)
            .then_with(|| right.citation.occurred_at.cmp(&left.citation.occurred_at))
            .then_with(|| right.citation.document_id.cmp(&left.citation.document_id))
    });
    results.truncate(limit);
    results
}

/// A gathered result as it is answered: the earliest recorded document cited, the others as its
/// copies, and its best passages.
fn into_result(mut gathered: Gathered) -> SearchResult {
    gathered.documents.sort_by(|left, right| {
        left.occurred_at
            .cmp(&right.occurred_at)
            .then_with(|| left.document_id.cmp(&right.document_id))
    });
    let mut documents = gathered.documents.into_iter();
    let citation = documents
        .next()
        .expect("every gathered result holds the document it was gathered from");
    let also_recorded_by = documents
        .map(|document| Copy {
            document_id: document.document_id,
            person: document.person,
        })
        .collect();

    let mut passages: Vec<(i32, (f64, String))> = gathered.passages.into_iter().collect();
    passages.sort_by(
        |(left_position, (left_score, _)), (right_position, (right_score, _))| {
            right_score
                .partial_cmp(left_score)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left_position.cmp(right_position))
        },
    );
    let passages = passages
        .into_iter()
        .take(PASSAGES_PER_RESULT)
        .map(|(position, (_, content))| Passage { position, content })
        .collect();

    SearchResult {
        citation,
        passages,
        also_recorded_by,
        score: gathered.score,
        pending_suggestions: 0,
    }
}

#[cfg(test)]
mod tests;
