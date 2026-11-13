// Copyright © 2026 Jalapeno Labs

//! What a session puts on its project's timeline: each turn's summary, so the project reads the
//! agents' work in order beside the meetings and messages about it, and the knowledge base finds
//! it as it finds every record.
//!
//! # One record per summary
//!
//! A summary of a session with a project is a timeline record of kind [`SOURCE_KIND`], written in
//! the transaction that records the summary (`super::summary`), and again by the backfill for the
//! summaries written before (`crate::knowledge::backfill`). The record names its summary
//! (`turn_summary_session_id`, `turn_summary_turn_id`), so writing it twice leaves one record
//! that reads as the summary does now, and deleting the session deletes it with the summary, as
//! the session's other history goes. A session without a project, which only a Studio session
//! can be, keeps its summaries and puts none on any timeline: a record on no project would read
//! as news on every member's whole timeline.
//!
//! # Whose it is
//!
//! Every session is listed to everyone, so its summaries are the workspace's, not its starter's:
//! the record is owned by the coding agent's machine user, which every company member reads on
//! the project's timeline (`timeline_record::SHARED_OWNERS`), and attributed to the project as
//! certain by the session's agent (`session:<n>`). Nothing wakes on it, and Elysia's intake leaves
//! it out: it is agents' work, followed in its own conversation.
//!
//! A turn that read a meeting can say so in its summary, so the record names every meeting its
//! turn read (`crate::models::meeting_reads`), and only whoever may read every one of them reads
//! it.
//!
//! # The record
//!
//! Its title is the summary, its actor the session (`Session 12`), and its body empty: the
//! summary is the whole of what it says. Its metadata names the session, the turn, and the Studio
//! item when the session is one's, which is how the web app opens the session at that turn.

use diesel_async::AsyncPgConnection;
use serde_json::json;
use tracing::{Level, event};
use uuid::Uuid;

use crate::models::attribution::{self, Attributor};
use crate::models::coding_session::CodingSession;
use crate::models::meeting_reads::{self, Turn};
use crate::models::timeline_record::{self, NewTimelineRecord, TimelineRecord};
use crate::models::turn_summary::TurnSummary;
use crate::models::user::CODING_AGENT_USER_ID;

/// The kind every turn summary's record is.
pub const SOURCE_KIND: &str = "turn_summary";

/// The version of what a summary's record holds. Bump it whenever [`record`] changes what a
/// record says, so the backfill writes every summary's record again.
pub const RECORDED_AS: i32 = 1;

/// Puts `summary`, a turn of `session`, on the session's project's timeline, or answers `None`
/// for a session without a project. Call it in the transaction that records the summary.
///
/// # Errors
/// Propagates any database error.
pub async fn record(
    connection: &mut AsyncPgConnection,
    summary: &TurnSummary,
    session: &CodingSession,
) -> diesel::QueryResult<Option<TimelineRecord>> {
    let Some(project_id) = session.project_id else {
        event!(
            name: "sessions.summary.no_project",
            Level::DEBUG,
            session.id = session.id,
            turn.id = %summary.turn_id,
            "a turn's summary is kept with its session, which is on no project's timeline",
        );
        return Ok(None);
    };
    let new_record = NewTimelineRecord {
        id: Uuid::now_v7(),
        source_kind: SOURCE_KIND.to_owned(),
        connector_id: None,
        external_id: Some(format!("turn:{}", summary.turn_id)),
        occurred_at: summary.created_at,
        actor: format!("Session {}", session.id),
        title: summary.summary.clone(),
        body: String::new(),
        url: None,
        media: None,
        metadata: json!({
            "sessionId": session.id,
            "turnId": summary.turn_id,
            "studioItemId": session.studio_item_id,
        }),
        created_by: CODING_AGENT_USER_ID,
    };
    let stored = timeline_record::upsert_turn_summary(
        connection,
        summary.session_id,
        summary.turn_id,
        new_record,
    )
    .await?;
    let read = meeting_reads::of_turn(connection, Turn::Session(summary.turn_id)).await?;
    meeting_reads::name_on_record(connection, stored.id, &read).await?;
    // A record written again is already on its project, which answers unchanged.
    attribution::attribute(
        connection,
        stored.id,
        project_id,
        attribution::PERSON_CONFIDENCE,
        Attributor::Session(session.id),
    )
    .await?;
    Ok(Some(stored))
}

#[cfg(test)]
mod tests {
    use diesel::prelude::*;
    use diesel_async::RunQueryDsl;

    use super::*;
    use crate::database::schema::{knowledge_documents, project_manager_wakes, timeline_records};
    use crate::models::coding_session::{self, NewCodingSession};
    use crate::models::meeting::{Reach, Reader};
    use crate::models::project::{self, NewProject};
    use crate::models::timeline_record::TimelineQuery;
    use crate::models::turn_summary;
    use crate::test_support::{TEST_PERSON_ID, cipher, migrated_database, studio_item};

    /// A Coding session the test person started on a new project, or with `on_project` false a
    /// Studio session on an item with none, the only kind of session that can lack one.
    async fn session(connection: &mut AsyncPgConnection, on_project: bool) -> CodingSession {
        let (project_id, studio_item_id) = if on_project {
            let project = project::create(
                connection,
                &NewProject {
                    created_by: TEST_PERSON_ID,
                    name: format!("Project {}", Uuid::now_v7()),
                    description: String::new(),
                },
            )
            .await
            .expect("a project");
            (Some(project.id), None)
        } else {
            (None, Some(studio_item(connection, &cipher()).await.id))
        };
        let id = coding_session::reserve_id(connection).await.expect("an id");
        coding_session::create(
            connection,
            &NewCodingSession {
                created_by: TEST_PERSON_ID,
                id,
                project_id,
                title: "Login form".to_owned(),
                github_credential_id: None,
                action_item_id: None,
                studio_item_id,
            },
        )
        .await
        .expect("a session")
    }

    /// The records of turn `turn_id`'s summary, as `(id, title)`.
    async fn records_of(connection: &mut AsyncPgConnection, turn_id: Uuid) -> Vec<(Uuid, String)> {
        timeline_records::table
            .filter(timeline_records::turn_summary_turn_id.eq(turn_id))
            .select((timeline_records::id, timeline_records::title))
            .load(connection)
            .await
            .expect("the records are read")
    }

    async fn document_count(connection: &mut AsyncPgConnection, record_id: Uuid) -> i64 {
        knowledge_documents::table
            .filter(knowledge_documents::timeline_record_id.eq(record_id))
            .count()
            .get_result(connection)
            .await
            .expect("the index is read")
    }

    // #193: one record per turn, however often its summary's record is written.
    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_summary_is_one_record_on_its_project_for_every_member_however_often_written() {
        let (_url, mut connection) = migrated_database().await;
        let session = session(&mut connection, true).await;
        let project_id = session.project_id.expect("a project");
        let turn_id = Uuid::now_v7();

        let first = turn_summary::record(&mut connection, session.id, turn_id, "Fixed the form.")
            .await
            .expect("recorded");
        let stored = record(&mut connection, &first, &session)
            .await
            .expect("stored")
            .expect("on its project");
        assert_eq!(stored.source_kind, SOURCE_KIND);
        assert_eq!(stored.created_by, CODING_AGENT_USER_ID);
        assert_eq!(stored.title, "Fixed the form.");
        assert_eq!(stored.metadata["sessionId"], session.id);
        assert_eq!(stored.metadata["turnId"], turn_id.to_string());
        assert_eq!(document_count(&mut connection, stored.id).await, 1);

        // Written again, as the backfill does, with the summary replaced meanwhile.
        let replaced = turn_summary::record(&mut connection, session.id, turn_id, "Fixed it.")
            .await
            .expect("replaced");
        let again = record(&mut connection, &replaced, &session)
            .await
            .expect("stored")
            .expect("on its project");
        assert_eq!(again.id, stored.id, "the same record");
        assert_eq!(
            records_of(&mut connection, turn_id).await,
            [(stored.id, "Fixed it.".to_owned())]
        );

        // Another member reads it on the project's timeline, and nothing woke its Project
        // Manager.
        let query = TimelineQuery {
            reader: Reader {
                id: Uuid::now_v7(),
                reach: Reach::Public,
            },
            shared: true,
            project_id: Some(project_id),
            action_item_id: None,
            meeting_id: None,
            before: None,
            limit: 10,
        };
        let listed = timeline_record::list(&mut connection, query)
            .await
            .expect("listed");
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].1[0].attributed_by,
            format!("session:{}", session.id)
        );
        let wakes: i64 = project_manager_wakes::table
            .count()
            .get_result(&mut connection)
            .await
            .expect("counted");
        assert_eq!(wakes, 0);

        // A guest reads only their own.
        let guests = TimelineQuery {
            reader: Reader {
                reach: Reach::Shared,
                ..query.reader
            },
            shared: false,
            ..query
        };
        assert!(
            timeline_record::list(&mut connection, guests)
                .await
                .expect("listed")
                .is_empty()
        );
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn a_session_without_a_project_keeps_its_summaries_off_every_timeline() {
        let (_url, mut connection) = migrated_database().await;
        let session = session(&mut connection, false).await;
        let turn_id = Uuid::now_v7();
        let summary = turn_summary::record(&mut connection, session.id, turn_id, "Drew a cat.")
            .await
            .expect("recorded");

        let stored = record(&mut connection, &summary, &session)
            .await
            .expect("answered");
        assert!(stored.is_none());
        assert!(records_of(&mut connection, turn_id).await.is_empty());
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn deleting_a_session_takes_its_summaries_records_and_their_documents_with_it() {
        let (_url, mut connection) = migrated_database().await;
        let session = session(&mut connection, true).await;
        let turn_id = Uuid::now_v7();
        let summary = turn_summary::record(&mut connection, session.id, turn_id, "Fixed it.")
            .await
            .expect("recorded");
        let stored = record(&mut connection, &summary, &session)
            .await
            .expect("stored")
            .expect("on its project");

        coding_session::delete(&mut connection, session.id)
            .await
            .expect("deleted");

        assert!(records_of(&mut connection, turn_id).await.is_empty());
        assert_eq!(document_count(&mut connection, stored.id).await, 0);
    }
}
