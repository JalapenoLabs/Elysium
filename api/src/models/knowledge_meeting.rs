// Copyright © 2026 Jalapeno Labs

//! The meetings the knowledge index holds, each with its topics in the order they were said.
//!
//! A meeting is in the index as its segments: each is a timeline record of its own
//! (`docs/meetings.md`, Segment records), and so a document of source `meeting`. [`list`]
//! gathers those documents back into their meetings by what every segment record names in its
//! metadata (`meetingId`, `meetingTitle`, `meetingSource`), so a reader finds a meeting by when
//! it was and opens its topics one by one, each a citation like any other.
//!
//! The `meetings` row itself (its transcript, recording, participants, and processing) stays its
//! owner's, and nothing here reads it: what a member may read is what the index holds
//! (`docs/knowledge.md`, Who sees what). Two people whose sources both recorded one meeting have
//! a meeting each, segmented apart, so both are listed.

use chrono::{DateTime, Utc};
use diesel::QueryableByName;
use diesel::result::QueryResult;
use diesel::sql_types::{BigInt, Nullable, Text, Timestamptz, Uuid as SqlUuid};
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use uuid::Uuid;

use crate::models::knowledge_document::{Citation, Person};

/// What a listing narrows to. Every filter narrows, and none is access control.
#[derive(Debug, Clone, Default)]
pub struct MeetingFilters {
    /// Only meetings recorded by this person's meeting sources.
    pub person_id: Option<Uuid>,
    /// Only meetings with at least one topic attributed to this project.
    pub project_id: Option<Uuid>,
    /// Only meetings that started at or after this moment.
    pub started_from: Option<DateTime<Utc>>,
    /// Only meetings that started before this moment.
    pub started_before: Option<DateTime<Utc>>,
}

/// One meeting as the index holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MeetingView {
    pub title: String,
    /// The meeting source it was read from, such as `fireflies`.
    pub source: String,
    /// When its first topic began: the meeting's start, or a little after it when nothing was
    /// said at first.
    pub started_at: DateTime<Utc>,
    /// Whose meeting source recorded it.
    pub person: Person,
    /// Its topics in the order they were said, each opened by its `documentId`.
    pub topics: Vec<Citation>,
}

/// One topic, with the meeting it belongs to.
#[derive(Debug, QueryableByName)]
struct TopicRow {
    #[diesel(sql_type = Text)]
    meeting_key: String,
    #[diesel(sql_type = Text)]
    meeting_title: String,
    #[diesel(sql_type = Text)]
    meeting_source: String,
    #[diesel(sql_type = Timestamptz)]
    started_at: DateTime<Utc>,
    #[diesel(sql_type = SqlUuid)]
    document_id: Uuid,
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

/// Every meeting topic in the index, then the newest `$5` meetings under the filters (`$1` the
/// person, `$2` and `$3` the period of its start, `$4` the project), then their topics in order.
/// Topics are ordered by when each began, which is the order the segmenting gave them.
const MEETINGS: &str = "
    WITH topics AS (
        SELECT documents.id AS document_id, documents.source, documents.title, documents.url,
            documents.occurred_at, documents.created_by, people.name AS person_name,
            records.metadata->>'meetingId' AS meeting_key,
            COALESCE(records.metadata->>'meetingTitle', '') AS meeting_title,
            COALESCE(records.metadata->>'meetingSource', '') AS meeting_source,
            EXISTS (
                SELECT 1 FROM timeline_record_projects attributed
                WHERE attributed.timeline_record_id = records.id
                    AND attributed.project_id = $4::uuid
            ) AS on_project
        FROM knowledge_documents documents
        JOIN timeline_records records ON records.id = documents.timeline_record_id
        JOIN users people ON people.id = documents.created_by
        WHERE documents.source = 'meeting'
            AND records.metadata->>'meetingId' IS NOT NULL
            AND ($1::uuid IS NULL OR documents.created_by = $1::uuid)
    ),
    meetings AS (
        SELECT meeting_key, min(occurred_at) AS started_at
        FROM topics
        GROUP BY meeting_key
        HAVING ($2::timestamptz IS NULL OR min(occurred_at) >= $2::timestamptz)
            AND ($3::timestamptz IS NULL OR min(occurred_at) < $3::timestamptz)
            AND ($4::uuid IS NULL OR bool_or(on_project))
        ORDER BY started_at DESC, meeting_key DESC
        LIMIT $5
    )
    SELECT topics.meeting_key, topics.meeting_title, topics.meeting_source, meetings.started_at,
        topics.document_id, topics.source, topics.title, topics.url, topics.occurred_at,
        topics.created_by, topics.person_name
    FROM topics
    JOIN meetings ON meetings.meeting_key = topics.meeting_key
    ORDER BY meetings.started_at DESC, meetings.meeting_key DESC, topics.occurred_at,
        topics.document_id";

/// The newest `limit` meetings in the index under `filters`, newest first, each with its topics
/// in order.
///
/// It answers any person's meetings: callers check that whoever asks may read the knowledge base
/// (`docs/knowledge.md`, Who sees what).
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    filters: &MeetingFilters,
    limit: i64,
) -> QueryResult<Vec<MeetingView>> {
    let rows: Vec<TopicRow> = diesel::sql_query(MEETINGS)
        .bind::<Nullable<SqlUuid>, _>(filters.person_id)
        .bind::<Nullable<Timestamptz>, _>(filters.started_from)
        .bind::<Nullable<Timestamptz>, _>(filters.started_before)
        .bind::<Nullable<SqlUuid>, _>(filters.project_id)
        .bind::<BigInt, _>(limit)
        .load(connection)
        .await?;
    Ok(gather(rows))
}

/// Folds topics, ordered meeting by meeting, into their meetings, keeping both orders.
fn gather(rows: Vec<TopicRow>) -> Vec<MeetingView> {
    let mut meetings: Vec<MeetingView> = Vec::new();
    let mut current_key: Option<String> = None;
    for row in rows {
        let topic = Citation {
            document_id: row.document_id,
            source: row.source,
            title: row.title,
            url: row.url,
            page_id: None,
            meeting_id: None,
            occurred_at: row.occurred_at,
            person: Person {
                id: row.created_by,
                name: row.person_name,
            },
        };
        let continues_meeting = current_key.as_deref() == Some(row.meeting_key.as_str());
        match meetings.last_mut() {
            Some(meeting) if continues_meeting => meeting.topics.push(topic),
            _ => {
                current_key = Some(row.meeting_key);
                meetings.push(MeetingView {
                    title: row.meeting_title,
                    source: row.meeting_source,
                    started_at: row.started_at,
                    person: topic.person.clone(),
                    topics: vec![topic],
                });
            }
        }
    }
    meetings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(meeting: &str, topic: &str, minutes: i64) -> TopicRow {
        let start = DateTime::from_timestamp(1_790_000_000, 0).expect("a fixed moment");
        TopicRow {
            meeting_key: meeting.to_owned(),
            meeting_title: format!("{meeting} sync"),
            meeting_source: "fireflies".to_owned(),
            started_at: start,
            document_id: Uuid::now_v7(),
            source: "meeting".to_owned(),
            title: topic.to_owned(),
            url: None,
            occurred_at: start + chrono::Duration::minutes(minutes),
            created_by: Uuid::nil(),
            person_name: "Ada".to_owned(),
        }
    }

    #[test]
    fn topics_gather_into_their_meetings_in_both_orders() {
        let meetings = gather(vec![
            row("b", "Launch", 0),
            row("b", "Hiring", 12),
            row("a", "Budget", 0),
        ]);

        let shape: Vec<(&str, Vec<&str>)> = meetings
            .iter()
            .map(|meeting| {
                let topics = meeting
                    .topics
                    .iter()
                    .map(|topic| topic.title.as_str())
                    .collect();
                (meeting.title.as_str(), topics)
            })
            .collect();
        assert_eq!(
            shape,
            [
                ("b sync", vec!["Launch", "Hiring"]),
                ("a sync", vec!["Budget"]),
            ]
        );
        assert_eq!(meetings[0].person.name, "Ada");
        assert_eq!(meetings[0].source, "fireflies");
    }

    #[test]
    fn no_topics_are_no_meetings() {
        assert!(gather(Vec::new()).is_empty());
    }
}
