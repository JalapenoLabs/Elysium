// Copyright © 2026 Jalapeno Labs

//! Questions people ask the knowledge base for a written answer, and the answers they get.
//!
//! An ask is made by the API ([`insert`]): the question, the filters it was narrowed to, and the
//! passages its search offered, by reference ([`PassageReference`]), in the order the prompt
//! labels them. The engine takes the queued ones ([`take_queued`]), loads what they offered
//! ([`offered_passages`]), and records how each ended ([`finish`] or [`fail`]). See
//! `crate::knowledge::ask` and `docs/knowledge.md`, Asking.
//!
//! An ask is its asker's alone: [`get_for`] and [`list_for`] answer only who asked. What it
//! cites is the company's, as every search result is.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{Array, BigInt, Integer, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::knowledge_asks;
use crate::models::knowledge_document::{Citation, Person};

/// Where an ask stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize)]
#[ExistingTypePath = "crate::database::schema::sql_types::KnowledgeAskState"]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeAskState {
    /// Waiting for the engine to take it.
    Queued,
    /// Its turn is writing the answer.
    Running,
    /// Answered, with every claim citing a passage the search offered.
    Answered,
    /// No answer could be written; the ask says why.
    Failed,
}

impl KnowledgeAskState {
    /// Whether nothing more will happen to it.
    pub const fn is_over(self) -> bool {
        matches!(self, Self::Answered | Self::Failed)
    }
}

/// One passage of the index, by the key it is stored under: its document and its place there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PassageReference {
    pub document_id: Uuid,
    pub position: i32,
}

/// A passage an ask's search offered, as its turn reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedPassage {
    /// Its place in what the search offered, from 1: the prompt labels it `P<label>`.
    pub label: usize,
    pub citation: Citation,
    pub position: i32,
    pub content: String,
}

/// What a source of an answer is: a passage it cites, numbered in the order the answer first
/// cites it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerSource {
    /// From 1, as the answer's claims cite it.
    pub number: usize,
    pub citation: Citation,
    /// The passage's place in its document, which `knowledge_get` and the document's page open
    /// it at.
    pub position: i32,
}

/// One statement of an answer and the sources that back it, by number. Never without one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnswerClaim {
    pub text: String,
    pub sources: Vec<usize>,
}

/// A checked answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Answer {
    /// In the order they read as an answer. None when nothing the search found answers it.
    pub claims: Vec<AnswerClaim>,
    /// Every passage a claim cites, by number.
    pub sources: Vec<AnswerSource>,
    /// How many statements the model wrote that cited no passage it was offered, and so were
    /// left out.
    pub unsupported_claims: usize,
}

#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = knowledge_asks)]
#[diesel(check_for_backend(diesel::pg::Pg))]
pub struct KnowledgeAsk {
    pub id: Uuid,
    pub created_by: Uuid,
    pub question: String,
    /// The filters as they were asked for, in their JSON form.
    pub filters: Value,
    /// [`PassageReference`]s, in the order the prompt labels them.
    pub passages: Value,
    pub searched_by_meaning: bool,
    pub state: KnowledgeAskState,
    /// An [`Answer`], once answered.
    pub answer: Option<Value>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

impl KnowledgeAsk {
    /// What its search offered, in label order.
    ///
    /// # Panics
    /// Panics when the stored list is not one [`insert`] wrote, which only a programming error
    /// or a hand-edited row can cause.
    pub fn offered(&self) -> Vec<PassageReference> {
        serde_json::from_value(self.passages.clone())
            .expect("an ask's passages are the references it was stored with")
    }
}

/// A new ask, as the API records it.
#[derive(Debug, Clone)]
pub struct NewKnowledgeAsk {
    pub id: Uuid,
    pub created_by: Uuid,
    pub question: String,
    pub filters: Value,
    /// What the search offered, best first. Empty when it found nothing, and then the ask is
    /// answered at once, with nothing to say, rather than spending a turn.
    pub offered: Vec<PassageReference>,
    pub searched_by_meaning: bool,
}

#[derive(Insertable)]
#[diesel(table_name = knowledge_asks)]
struct NewRow {
    id: Uuid,
    created_by: Uuid,
    question: String,
    filters: Value,
    passages: Value,
    searched_by_meaning: bool,
    state: KnowledgeAskState,
    answer: Option<Value>,
    finished_at: Option<DateTime<Utc>>,
}

/// Records `ask`: queued for the engine, or answered with nothing to say when its search
/// offered nothing.
///
/// # Errors
/// Propagates any database error, including a check violation for a question outside its
/// length.
pub async fn insert(
    connection: &mut AsyncPgConnection,
    ask: NewKnowledgeAsk,
) -> QueryResult<KnowledgeAsk> {
    let found_nothing = ask.offered.is_empty();
    let (state, answer, finished_at) = if found_nothing {
        (
            KnowledgeAskState::Answered,
            Some(answer_json(&Answer::default())),
            Some(Utc::now()),
        )
    } else {
        (KnowledgeAskState::Queued, None, None)
    };
    let row = NewRow {
        id: ask.id,
        created_by: ask.created_by,
        question: ask.question,
        filters: ask.filters,
        passages: serde_json::to_value(&ask.offered).expect("references serialize"),
        searched_by_meaning: ask.searched_by_meaning,
        state,
        answer,
        finished_at,
    };
    diesel::insert_into(knowledge_asks::table)
        .values(row)
        .returning(KnowledgeAsk::as_returning())
        .get_result(connection)
        .await
}

/// `answer` as it is stored.
fn answer_json(answer: &Answer) -> Value {
    serde_json::to_value(answer).expect("an answer serializes")
}

/// Ask `id`, when `asker` asked it.
///
/// # Errors
/// Propagates any database error.
pub async fn get_for(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    asker: Uuid,
) -> QueryResult<Option<KnowledgeAsk>> {
    knowledge_asks::table
        .filter(knowledge_asks::id.eq(id))
        .filter(knowledge_asks::created_by.eq(asker))
        .select(KnowledgeAsk::as_select())
        .first(connection)
        .await
        .optional()
}

/// `asker`'s newest `limit` asks, newest first.
///
/// # Errors
/// Propagates any database error.
pub async fn list_for(
    connection: &mut AsyncPgConnection,
    asker: Uuid,
    limit: i64,
) -> QueryResult<Vec<KnowledgeAsk>> {
    knowledge_asks::table
        .filter(knowledge_asks::created_by.eq(asker))
        .order((knowledge_asks::created_at.desc(), knowledge_asks::id.desc()))
        .limit(limit)
        .select(KnowledgeAsk::as_select())
        .load(connection)
        .await
}

/// What one person has spent of their asks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, QueryableByName)]
pub struct Spent {
    /// How many they asked since the window's start.
    #[diesel(sql_type = BigInt)]
    pub in_window: i64,
    /// When the oldest of those was asked, which is when the window frees one.
    #[diesel(sql_type = Nullable<Timestamptz>)]
    pub oldest_in_window: Option<DateTime<Utc>>,
    /// How many of theirs are still queued or running.
    #[diesel(sql_type = BigInt)]
    pub waiting: i64,
}

/// Holds `asker`'s asks until the transaction ends, so two asks made at once count each other:
/// what [`spent_by`] reads under it stays true until the ask it admits is inserted.
///
/// # Errors
/// Propagates any database error.
pub async fn lock_asks_of(connection: &mut AsyncPgConnection, asker: Uuid) -> QueryResult<()> {
    diesel::sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind::<Text, _>(format!("knowledge_asks:{asker}"))
        .execute(connection)
        .await
        .map(|_locked| ())
}

/// What `asker` has spent of their asks since `since`, and how many still wait for an answer.
///
/// # Errors
/// Propagates any database error.
pub async fn spent_by(
    connection: &mut AsyncPgConnection,
    asker: Uuid,
    since: DateTime<Utc>,
) -> QueryResult<Spent> {
    diesel::sql_query(
        "SELECT count(*) FILTER (WHERE created_at >= $2) AS in_window,
             min(created_at) FILTER (WHERE created_at >= $2) AS oldest_in_window,
             count(*) FILTER (WHERE state IN ('queued', 'running')) AS waiting
         FROM knowledge_asks
         WHERE created_by = $1 AND (created_at >= $2 OR state IN ('queued', 'running'))",
    )
    .bind::<SqlUuid, _>(asker)
    .bind::<Timestamptz, _>(since)
    .get_result(connection)
    .await
}

/// Takes up to `limit` queued asks, oldest first, marking them running.
///
/// # Errors
/// Propagates any database error.
pub async fn take_queued(
    connection: &mut AsyncPgConnection,
    limit: i64,
) -> QueryResult<Vec<KnowledgeAsk>> {
    // One engine runs, so nothing else takes asks; skipping locked rows keeps this correct
    // should that change.
    let mut taken: Vec<KnowledgeAsk> = connection
        .transaction(async move |connection| {
            let waiting: Vec<Uuid> = knowledge_asks::table
                .filter(knowledge_asks::state.eq(KnowledgeAskState::Queued))
                .order((knowledge_asks::created_at.asc(), knowledge_asks::id.asc()))
                .limit(limit)
                .select(knowledge_asks::id)
                .for_update()
                .skip_locked()
                .load(connection)
                .await?;
            if waiting.is_empty() {
                return Ok(Vec::new());
            }
            diesel::update(knowledge_asks::table.filter(knowledge_asks::id.eq_any(&waiting)))
                .set((
                    knowledge_asks::state.eq(KnowledgeAskState::Running),
                    knowledge_asks::started_at.eq(Utc::now()),
                ))
                .returning(KnowledgeAsk::as_returning())
                .get_results(connection)
                .await
        })
        .await?;
    // `RETURNING` keeps no order, and the oldest is answered first.
    taken.sort_by_key(|ask| (ask.created_at, ask.id));
    Ok(taken)
}

/// Records `answer` for running ask `id`; `None` when it is not running any more.
///
/// # Errors
/// Propagates any database error.
pub async fn finish(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    answer: &Answer,
) -> QueryResult<Option<KnowledgeAsk>> {
    diesel::update(
        knowledge_asks::table
            .filter(knowledge_asks::id.eq(id))
            .filter(knowledge_asks::state.eq(KnowledgeAskState::Running)),
    )
    .set((
        knowledge_asks::state.eq(KnowledgeAskState::Answered),
        knowledge_asks::answer.eq(answer_json(answer)),
        knowledge_asks::finished_at.eq(Utc::now()),
    ))
    .returning(KnowledgeAsk::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// The longest error kept, as `knowledge_asks.error` allows.
const ERROR_MAX_CHARACTERS: usize = 2_000;

/// Fails ask `id`, queued or running, saying `error`; `None` when it had already ended.
///
/// # Errors
/// Propagates any database error.
pub async fn fail(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    error: &str,
) -> QueryResult<Option<KnowledgeAsk>> {
    diesel::update(
        knowledge_asks::table
            .filter(knowledge_asks::id.eq(id))
            .filter(
                knowledge_asks::state
                    .eq_any([KnowledgeAskState::Queued, KnowledgeAskState::Running]),
            ),
    )
    .set((
        knowledge_asks::state.eq(KnowledgeAskState::Failed),
        knowledge_asks::error.eq(stored_error(error)),
        knowledge_asks::finished_at.eq(Utc::now()),
    ))
    .returning(KnowledgeAsk::as_returning())
    .get_result(connection)
    .await
    .optional()
}

/// Fails every running ask, saying `error`: what an engine that stopped left behind, since
/// nothing is writing their answers any more.
///
/// # Errors
/// Propagates any database error.
pub async fn fail_running(
    connection: &mut AsyncPgConnection,
    error: &str,
) -> QueryResult<Vec<KnowledgeAsk>> {
    diesel::update(
        knowledge_asks::table.filter(knowledge_asks::state.eq(KnowledgeAskState::Running)),
    )
    .set((
        knowledge_asks::state.eq(KnowledgeAskState::Failed),
        knowledge_asks::error.eq(stored_error(error)),
        knowledge_asks::finished_at.eq(Utc::now()),
    ))
    .returning(KnowledgeAsk::as_returning())
    .get_results(connection)
    .await
}

/// `error`, never empty and within what the column allows.
fn stored_error(error: &str) -> String {
    let trimmed = error.trim();
    if trimmed.is_empty() {
        return "The answer could not be written".to_owned();
    }
    trimmed.chars().take(ERROR_MAX_CHARACTERS).collect()
}

/// One offered passage with what its citation names, as [`offered_passages`] reads it.
#[derive(Debug, QueryableByName)]
struct OfferedRow {
    #[diesel(sql_type = Integer)]
    label: i32,
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
    #[diesel(sql_type = Timestamptz)]
    occurred_at: DateTime<Utc>,
    #[diesel(sql_type = SqlUuid)]
    created_by: Uuid,
    #[diesel(sql_type = Text)]
    person_name: String,
}

/// The passages `offered` names that are still in the index, in label order, each labelled
/// by its place in `offered`, from 1. One whose record was removed since is left out, so its
/// label is offered to nobody.
///
/// # Errors
/// Propagates any database error.
pub async fn offered_passages(
    connection: &mut AsyncPgConnection,
    offered: &[PassageReference],
) -> QueryResult<Vec<OfferedPassage>> {
    let document_ids: Vec<Uuid> = offered
        .iter()
        .map(|reference| reference.document_id)
        .collect();
    let positions: Vec<i32> = offered.iter().map(|reference| reference.position).collect();
    let rows: Vec<OfferedRow> = diesel::sql_query(
        "SELECT offered.label::integer AS label, passages.document_id, passages.position,
             passages.content, documents.source, documents.title, documents.url,
             documents.occurred_at, documents.created_by, people.name AS person_name
         FROM unnest($1::uuid[], $2::integer[]) WITH ORDINALITY
             AS offered (document_id, position, label)
         JOIN knowledge_passages passages
             ON passages.document_id = offered.document_id
             AND passages.position = offered.position
         JOIN knowledge_documents documents ON documents.id = passages.document_id
         JOIN users people ON people.id = documents.created_by
         ORDER BY offered.label",
    )
    .bind::<Array<SqlUuid>, _>(document_ids)
    .bind::<Array<Integer>, _>(positions)
    .load(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| OfferedPassage {
            label: usize::try_from(row.label).expect("ordinality counts from 1"),
            citation: Citation {
                document_id: row.document_id,
                source: row.source,
                title: row.title,
                url: row.url,
                occurred_at: row.occurred_at,
                person: Person {
                    id: row.created_by,
                    name: row.person_name,
                },
            },
            position: row.position,
            content: row.content,
        })
        .collect())
}
