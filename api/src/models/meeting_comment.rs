// Copyright © 2026 Jalapeno Labs

//! What is written in a meeting's comment threads, whom each comment mentions, and the inbox items
//! a thread raised. See `docs/meetings.md`, Comments.
//!
//! A thread is a mark of kind `comment` over its range (`models::meeting_mark`): the mark draws
//! it, moves with an edit, and is made, removed, resolved, and reopened by its own history
//! entries. Its comments are rows here, oldest first, each naming who wrote it and whom it
//! mentions. A removed thread keeps its comments, so an undo puts it back whole. What a comment
//! sets off (who is told, Elysia's turn, the index) is `meetings::comments`'.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::sql_types::{BigInt, Text, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use crate::database::schema::{
    meeting_comment_mentions, meeting_comments, meeting_marks, meeting_thread_items, users,
};
use crate::models::agent::Author;

/// The longest comment, in characters, as `meeting_comments.body` allows: an action item's
/// comment's limit, so a thought written in one fits in the other.
pub const BODY_MAX_CHARACTERS: usize = 20_000;

/// How much of a thread's range the index quotes beside its comments: enough to say which words
/// it is about, never the whole meeting a range can cover.
const QUOTE_MAX_WORDS: i64 = 40;

/// One comment in a thread.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable)]
#[diesel(table_name = meeting_comments, check_for_backend(diesel::pg::Pg))]
pub struct Comment {
    pub id: Uuid,
    /// The thread: its comment mark.
    pub mark_id: Uuid,
    pub body: String,
    pub created_by: Uuid,
    pub agent: Option<String>,
    pub agent_name: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// A comment with whom it mentions, in the order they were named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenComment {
    pub comment: Comment,
    pub mentions: Vec<Uuid>,
}

#[derive(Debug, Insertable)]
#[diesel(table_name = meeting_comments)]
struct NewComment<'body> {
    id: Uuid,
    mark_id: Uuid,
    body: &'body str,
    created_by: Uuid,
    agent: Option<String>,
    agent_name: Option<String>,
    created_at: DateTime<Utc>,
}

/// Writes `body` in thread `mark_id` as `author`, mentioning `mentions`, and answers it. The
/// caller has checked the thread stands and whom it may mention.
///
/// # Errors
/// Propagates any database error, such as a body the table refuses.
pub(crate) async fn insert(
    connection: &mut AsyncPgConnection,
    mark_id: Uuid,
    body: &str,
    mentions: &[Uuid],
    author: Author<'_>,
) -> QueryResult<WrittenComment> {
    let comment = diesel::insert_into(meeting_comments::table)
        .values(NewComment {
            id: Uuid::now_v7(),
            mark_id,
            body,
            created_by: author.user_id,
            agent: author.agent.map(|agent| agent.id().to_owned()),
            agent_name: author
                .agent
                .and_then(|agent| agent.name().map(ToOwned::to_owned)),
            created_at: author.at,
        })
        .returning(Comment::as_returning())
        .get_result(connection)
        .await?;
    if !mentions.is_empty() {
        let rows: Vec<_> = mentions
            .iter()
            .map(|user_id| {
                (
                    meeting_comment_mentions::comment_id.eq(comment.id),
                    meeting_comment_mentions::user_id.eq(*user_id),
                )
            })
            .collect();
        diesel::insert_into(meeting_comment_mentions::table)
            .values(rows)
            .execute(connection)
            .await?;
    }
    Ok(WrittenComment {
        comment,
        mentions: mentions.to_vec(),
    })
}

/// Every comment in meeting `meeting_id`'s standing threads, each thread's oldest first, the
/// threads in the order they were started.
///
/// # Errors
/// Propagates any database error.
pub async fn of_meeting(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
) -> QueryResult<Vec<WrittenComment>> {
    let comments: Vec<Comment> = meeting_comments::table
        .inner_join(meeting_marks::table)
        .filter(meeting_marks::meeting_id.eq(meeting_id))
        .filter(meeting_marks::removed_by.is_null())
        .order((meeting_comments::mark_id.asc(), meeting_comments::id.asc()))
        .select(Comment::as_select())
        .load(connection)
        .await?;
    with_mentions(connection, comments).await
}

/// Thread `mark_id`'s comments, oldest first.
///
/// # Errors
/// Propagates any database error.
pub async fn of_thread(
    connection: &mut AsyncPgConnection,
    mark_id: Uuid,
) -> QueryResult<Vec<WrittenComment>> {
    let comments: Vec<Comment> = meeting_comments::table
        .filter(meeting_comments::mark_id.eq(mark_id))
        .order(meeting_comments::id.asc())
        .select(Comment::as_select())
        .load(connection)
        .await?;
    with_mentions(connection, comments).await
}

/// `comments`, in their order, each with whom it mentions, read in one query.
async fn with_mentions(
    connection: &mut AsyncPgConnection,
    comments: Vec<Comment>,
) -> QueryResult<Vec<WrittenComment>> {
    if comments.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<Uuid> = comments.iter().map(|comment| comment.id).collect();
    let rows: Vec<(Uuid, Uuid)> = meeting_comment_mentions::table
        .filter(meeting_comment_mentions::comment_id.eq_any(&ids))
        .order((
            meeting_comment_mentions::comment_id.asc(),
            meeting_comment_mentions::user_id.asc(),
        ))
        .select((
            meeting_comment_mentions::comment_id,
            meeting_comment_mentions::user_id,
        ))
        .load(connection)
        .await?;
    let mut mentions: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for (comment_id, user_id) in rows {
        mentions.entry(comment_id).or_default().push(user_id);
    }
    Ok(comments
        .into_iter()
        .map(|comment| WrittenComment {
            mentions: mentions.remove(&comment.id).unwrap_or_default(),
            comment,
        })
        .collect())
}

/// Everyone taking part in thread `mark_id`: whoever wrote in it or was mentioned in it, in the
/// order they first did.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn participants(
    connection: &mut AsyncPgConnection,
    mark_id: Uuid,
) -> QueryResult<Vec<Uuid>> {
    let mut seen = HashSet::new();
    let mut participants = Vec::new();
    for written in of_thread(connection, mark_id).await? {
        let named = std::iter::once(written.comment.created_by).chain(written.mentions);
        for user_id in named {
            if seen.insert(user_id) {
                participants.push(user_id);
            }
        }
    }
    Ok(participants)
}

/// Records that thread `mark_id` raised inbox item `action_item_id` for `person_id`, by a change
/// of `created_by`'s.
///
/// # Errors
/// Propagates any database error.
pub(crate) async fn record_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
    mark_id: Uuid,
    person_id: Uuid,
    created_by: Uuid,
) -> QueryResult<()> {
    diesel::insert_into(meeting_thread_items::table)
        .values((
            meeting_thread_items::action_item_id.eq(action_item_id),
            meeting_thread_items::mark_id.eq(mark_id),
            meeting_thread_items::person_id.eq(person_id),
            meeting_thread_items::created_by.eq(created_by),
        ))
        .execute(connection)
        .await
        .map(drop)
}

/// The thread an inbox item was raised by, as its meeting and its mark, when one raised it.
///
/// # Errors
/// Propagates any database error.
pub async fn thread_of_item(
    connection: &mut AsyncPgConnection,
    action_item_id: Uuid,
) -> QueryResult<Option<(Uuid, Uuid)>> {
    meeting_thread_items::table
        .inner_join(meeting_marks::table)
        .filter(meeting_thread_items::action_item_id.eq(action_item_id))
        .select((meeting_marks::meeting_id, meeting_marks::id))
        .first(connection)
        .await
        .optional()
}

/// One thread as the knowledge index holds it, beside the excerpt its range lies in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedThread {
    /// The first words of its range, as the transcript reads now.
    pub quote: String,
    /// Its comments, oldest first, each by its author's name.
    pub comments: Vec<IndexedComment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedComment {
    pub author: String,
    pub body: String,
}

#[derive(Debug, QueryableByName)]
struct ThreadRow {
    #[diesel(sql_type = SqlUuid)]
    mark_id: Uuid,
    #[diesel(sql_type = Text)]
    quote: String,
}

#[derive(Debug, QueryableByName)]
struct SpanRow {
    #[diesel(sql_type = BigInt)]
    start_ms: i64,
    #[diesel(sql_type = BigInt)]
    end_ms: i64,
}

/// The standing threads of meeting `$1` whose range overlaps `$2` to `$3` on its clock, as the
/// tables `marks`, `first` (its first word), and `last` (its last word), for both queries over
/// them to share one predicate. A range overlaps the span when it starts before the span ends and
/// ends after it starts; a range of one instant, a word with no time of its own, counts as a
/// millisecond long, so it still lies in the excerpt around it.
const THREADS_OVER: &str = "
    FROM meeting_marks marks
    JOIN meeting_words first ON first.id = marks.start_word_id
    JOIN meeting_words last ON last.id = marks.end_word_id
    WHERE marks.meeting_id = $1
      AND marks.kind = 'comment'
      AND marks.removed_by IS NULL
      AND first.start_ms < $3
      AND GREATEST(last.end_ms, first.start_ms + 1) > $2";

/// Where on its clock each of meeting `meeting_id`'s standing threads overlapping `start_ms` to
/// `end_ms` lies, from its first word's start to its last word's end: the spans whose excerpts a
/// change to those words indexes again.
///
/// # Errors
/// Propagates any database error.
pub async fn spans_over(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    start_ms: i64,
    end_ms: i64,
) -> QueryResult<Vec<(i64, i64)>> {
    let rows: Vec<SpanRow> = diesel::sql_query(format!(
        "SELECT first.start_ms, GREATEST(last.end_ms, first.start_ms) AS end_ms {THREADS_OVER}"
    ))
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<BigInt, _>(start_ms)
    .bind::<BigInt, _>(end_ms)
    .load(connection)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.start_ms, row.end_ms))
        .collect())
}

/// Meeting `meeting_id`'s standing threads whose range overlaps `start_ms` to `end_ms` on its
/// clock, in the order their ranges start, each with its comments: what an excerpt over that
/// span indexes beside its own text (`knowledge::records`).
///
/// # Errors
/// Propagates any database error.
pub async fn indexed_over(
    connection: &mut AsyncPgConnection,
    meeting_id: Uuid,
    start_ms: i64,
    end_ms: i64,
) -> QueryResult<Vec<IndexedThread>> {
    // The quote reads the standing words between its ends, through the transcript's own index,
    // leaving out every word a standing black-out covers, as everything indexed does.
    let threads: Vec<ThreadRow> = diesel::sql_query(format!(
        "SELECT marks.id AS mark_id,
                COALESCE((
                    SELECT string_agg(quoted.text, ' ' ORDER BY quoted.position)
                    FROM (
                        SELECT words.text, words.position
                        FROM meeting_words words
                        WHERE words.meeting_id = marks.meeting_id
                          AND words.superseded_by IS NULL
                          AND words.position BETWEEN first.position AND last.position
                          AND NOT EXISTS (
                              SELECT 1
                              FROM meeting_marks black_outs
                              JOIN meeting_words black_out_first
                                ON black_out_first.id = black_outs.start_word_id
                              JOIN meeting_words black_out_last
                                ON black_out_last.id = black_outs.end_word_id
                              WHERE black_outs.meeting_id = marks.meeting_id
                                AND black_outs.kind = 'black_out'
                                AND black_outs.removed_by IS NULL
                                AND words.position
                                    BETWEEN black_out_first.position AND black_out_last.position
                          )
                        ORDER BY words.position
                        LIMIT $4
                    ) quoted
                ), '') AS quote
         {THREADS_OVER}
         ORDER BY first.position, marks.id",
    ))
    .bind::<SqlUuid, _>(meeting_id)
    .bind::<BigInt, _>(start_ms)
    .bind::<BigInt, _>(end_ms)
    .bind::<BigInt, _>(QUOTE_MAX_WORDS)
    .load(connection)
    .await?;
    if threads.is_empty() {
        return Ok(Vec::new());
    }

    let mark_ids: Vec<Uuid> = threads.iter().map(|thread| thread.mark_id).collect();
    let comments: Vec<(Uuid, String, String)> = meeting_comments::table
        .inner_join(users::table)
        .filter(meeting_comments::mark_id.eq_any(&mark_ids))
        .order((meeting_comments::mark_id.asc(), meeting_comments::id.asc()))
        .select((
            meeting_comments::mark_id,
            users::name,
            meeting_comments::body,
        ))
        .load(connection)
        .await?;
    let mut by_thread: HashMap<Uuid, Vec<IndexedComment>> = HashMap::new();
    for (mark_id, author, body) in comments {
        by_thread
            .entry(mark_id)
            .or_default()
            .push(IndexedComment { author, body });
    }
    Ok(threads
        .into_iter()
        .map(|thread| IndexedThread {
            comments: by_thread.remove(&thread.mark_id).unwrap_or_default(),
            quote: thread.quote,
        })
        .collect())
}
