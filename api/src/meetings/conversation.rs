// Copyright © 2026 Jalapeno Labs

//! A conversation as segmenting reads it, and its topics' projects on its messages. See
//! `super::segment`.
//!
//! A conversation is messages already on its person's timeline (Slack, Teams, mail), so it is
//! read as a transcript of them: each message a line, timed from the first, by its sender.
//! Its topics are runs of those messages, and each message is attributed to its topic's projects.
//! The reading replaces what Elysia said of those messages before: an attribution of hers that
//! the new reading does not make is taken off, so a conversation read again as it grows never
//! keeps a guess its fuller reading dropped. A person's attributions are never touched
//! (`models::attribution`).

use std::collections::{HashMap, HashSet};

use diesel::QueryResult;
use diesel_async::AsyncPgConnection;
use uuid::Uuid;

use super::segmentation::{ConversationSegment, SegmentProject};
use super::source::TranscriptLine;
use crate::models::attribution::{self, Attributor, ELYSIA_ACTOR};
use crate::models::timeline_record::{self, TimelineRecord};

/// `records`, in the order they were said, as the lines of a transcript: each timed from the
/// first, spoken by its sender, saying its body, or its title when it has none, such as a mail
/// read only by its subject.
pub fn lines_of(records: &[TimelineRecord]) -> Vec<TranscriptLine> {
    let Some(first) = records.first() else {
        return Vec::new();
    };
    records
        .iter()
        .map(|record| {
            let offset_ms = (record.occurred_at - first.occurred_at).num_milliseconds();
            let text = if record.body.trim().is_empty() {
                record.title.clone()
            } else {
                record.body.clone()
            };
            TranscriptLine {
                start_ms: offset_ms,
                end_ms: offset_ms,
                speaker: record.actor.clone(),
                text,
            }
        })
        .collect()
}

/// Attributes each of `records` (in the order the segments number them) to the projects of the
/// topic it is in, among `existing`, and takes off each of Elysia's attributions of them that
/// the topics do not make. Answers the records whose attributions changed.
///
/// # Errors
/// Propagates any database error.
pub async fn attribute(
    connection: &mut AsyncPgConnection,
    records: &[TimelineRecord],
    segments: &[ConversationSegment],
    existing: &HashSet<Uuid>,
) -> QueryResult<Vec<Uuid>> {
    let ids: Vec<Uuid> = records.iter().map(|record| record.id).collect();
    let mut elysias: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
    for held in timeline_record::attributions_of(connection, &ids).await? {
        if held.attributed_by == ELYSIA_ACTOR {
            elysias
                .entry(held.timeline_record_id)
                .or_default()
                .push(held.project_id);
        }
    }

    let mut changed: Vec<Uuid> = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let wanted: Vec<&SegmentProject> = segments
            .iter()
            .find(|segment| (segment.first..=segment.last).contains(&index))
            .map(|segment| {
                segment
                    .projects
                    .iter()
                    .filter(|project| existing.contains(&project.project_id))
                    .collect()
            })
            .unwrap_or_default();

        let mut moved = false;
        for project_id in elysias.remove(&record.id).unwrap_or_default() {
            let kept = wanted
                .iter()
                .any(|project| project.project_id == project_id);
            if kept {
                continue;
            }
            let written =
                attribution::unattribute(connection, record.id, project_id, Attributor::Elysia)
                    .await?;
            moved |= written.entry().is_some();
        }
        for project in wanted {
            let written = attribution::attribute(
                connection,
                record.id,
                project.project_id,
                project.confidence,
                Attributor::Elysia,
            )
            .await?;
            moved |= written.entry().is_some();
        }
        if moved {
            changed.push(record.id);
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeDelta, Utc};
    use serde_json::json;

    use super::*;
    use crate::models::project::{self, NewProject};
    use crate::models::timeline_record::NewTimelineRecord;
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    fn message(seconds: i64, actor: &str, title: &str, body: &str) -> NewTimelineRecord {
        let base: DateTime<Utc> = "2026-11-04T12:00:00Z".parse().expect("a time");
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "slack".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: base + TimeDelta::seconds(seconds),
            actor: actor.to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            url: None,
            media: None,
            metadata: json!({}),
            created_by: TEST_PERSON_ID,
        }
    }

    fn stored(new: NewTimelineRecord) -> TimelineRecord {
        TimelineRecord {
            id: new.id,
            source_kind: new.source_kind,
            connector_id: new.connector_id,
            external_id: new.external_id,
            occurred_at: new.occurred_at,
            actor: new.actor,
            title: new.title,
            body: new.body,
            url: new.url,
            media: new.media,
            metadata: new.metadata,
            action_item_id: None,
            created_by: new.created_by,
            created_at: Utc::now(),
        }
    }

    #[test]
    fn a_conversation_reads_as_lines_timed_from_its_first_message() {
        let records = [
            stored(message(0, "Ada", "#launch", "Can we ship Friday?")),
            stored(message(95, "Grace", "Re: Launch", " ")),
        ];
        let lines = lines_of(&records);

        assert_eq!(
            lines,
            vec![
                TranscriptLine {
                    start_ms: 0,
                    end_ms: 0,
                    speaker: "Ada".to_owned(),
                    text: "Can we ship Friday?".to_owned(),
                },
                // A mail read by its subject alone says its subject.
                TranscriptLine {
                    start_ms: 95_000,
                    end_ms: 95_000,
                    speaker: "Grace".to_owned(),
                    text: "Re: Launch".to_owned(),
                },
            ]
        );
        assert!(lines_of(&[]).is_empty());
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_conversations_reading_replaces_elysias_and_leaves_a_persons() {
        let (_url, mut connection) = migrated_database().await;
        let mut projects = Vec::new();
        for name in ["Website", "Launch"] {
            let created = project::create(
                &mut connection,
                &NewProject {
                    name: name.to_owned(),
                    description: String::new(),
                    created_by: TEST_PERSON_ID,
                },
            )
            .await
            .expect("a project");
            projects.push(created.id);
        }
        let (website, launch) = (projects[0], projects[1]);
        let mut records = Vec::new();
        for (seconds, body) in [(0, "Can we ship Friday?"), (30, "Yes, Friday.")] {
            let new = message(seconds, "Ada", "#launch", body);
            let record = timeline_record::insert_new(&mut connection, new)
                .await
                .expect("stored")
                .expect("new");
            records.push(record);
        }
        // An earlier, shorter reading guessed the website; the person put the reply there too.
        attribution::attribute(
            &mut connection,
            records[0].id,
            website,
            0.6,
            Attributor::Elysia,
        )
        .await
        .expect("attributes");
        attribution::attribute(
            &mut connection,
            records[1].id,
            website,
            1.0,
            Attributor::Person(TEST_PERSON_ID),
        )
        .await
        .expect("attributes");

        let segments = [ConversationSegment {
            first: 0,
            last: 1,
            title: "Launch".to_owned(),
            summary: "Ships Friday.".to_owned(),
            projects: vec![SegmentProject {
                project_id: launch,
                confidence: 0.9,
            }],
        }];
        let existing: HashSet<Uuid> = projects.iter().copied().collect();
        let changed = attribute(&mut connection, &records, &segments, &existing)
            .await
            .expect("attributes");
        assert_eq!(changed, vec![records[0].id, records[1].id]);

        let mut held: Vec<(Uuid, Uuid, String)> =
            timeline_record::attributions_of(&mut connection, &[records[0].id, records[1].id])
                .await
                .expect("reads")
                .into_iter()
                .map(|held| (held.timeline_record_id, held.project_id, held.attributed_by))
                .collect();
        held.sort();
        let mut expected = vec![
            // Elysia's earlier guess gave way to her fuller reading.
            (records[0].id, launch, ELYSIA_ACTOR.to_owned()),
            (records[1].id, launch, ELYSIA_ACTOR.to_owned()),
            // The person's stays.
            (records[1].id, website, format!("user:{TEST_PERSON_ID}")),
        ];
        expected.sort();
        assert_eq!(held, expected);

        // Read again as it stands, nothing changes.
        let again = attribute(&mut connection, &records, &segments, &existing)
            .await
            .expect("attributes");
        assert!(again.is_empty());
    }
}
