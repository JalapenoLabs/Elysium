// Copyright © 2026 Jalapeno Labs

//! The timeline: everything Elysium ingests, as one kind of record, attributed to projects.
//! See `docs/connectors.md`.
//!
//! A record belongs to whoever it was ingested for, and only they list it: a person's mail is
//! theirs even when it is about a shared project.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel::upsert::excluded;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::database::schema::{timeline_record_projects, timeline_records};

#[derive(Debug, Clone, Queryable, Selectable, Identifiable)]
#[diesel(table_name = timeline_records, check_for_backend(diesel::pg::Pg))]
pub struct TimelineRecord {
    pub id: Uuid,
    pub source_kind: String,
    pub connector_account_id: Option<Uuid>,
    pub external_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub actor: String,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub media: Option<Value>,
    pub metadata: Value,
    pub action_item_id: Option<Uuid>,
    pub created_by: Uuid,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = timeline_records)]
pub struct NewTimelineRecord {
    pub id: Uuid,
    pub source_kind: String,
    pub connector_account_id: Option<Uuid>,
    pub external_id: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub actor: String,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub media: Option<Value>,
    pub metadata: Value,
    pub created_by: Uuid,
}

/// Which projects a record is about, with how sure the attribution is.
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = timeline_record_projects, check_for_backend(diesel::pg::Pg))]
pub struct Attribution {
    pub timeline_record_id: Uuid,
    pub project_id: Uuid,
    pub confidence: f32,
    pub attributed_by: String,
    pub created_at: DateTime<Utc>,
}

/// Stores `record` unless its account already recorded the same external id, and returns it
/// when it is new. Reading the same message twice therefore records it once.
///
/// # Errors
/// Propagates any database error.
pub async fn insert_new(
    connection: &mut AsyncPgConnection,
    record: NewTimelineRecord,
) -> QueryResult<Option<TimelineRecord>> {
    diesel::insert_into(timeline_records::table)
        .values(record)
        .on_conflict((
            timeline_records::connector_account_id,
            timeline_records::external_id,
        ))
        .do_nothing()
        .returning(TimelineRecord::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// Records the action item a record raised.
///
/// # Errors
/// Propagates any database error.
pub async fn set_action_item(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    action_item_id: Uuid,
) -> QueryResult<TimelineRecord> {
    diesel::update(timeline_records::table.find(id))
        .set(timeline_records::action_item_id.eq(action_item_id))
        .returning(TimelineRecord::as_returning())
        .get_result(connection)
        .await
}

/// What to list from the timeline.
#[derive(Debug, Clone, Copy)]
pub struct TimelineQuery {
    pub owner: Uuid,
    /// Only records attributed to this project.
    pub project_id: Option<Uuid>,
    /// Only records that happened before this, for the next page.
    pub before: Option<DateTime<Utc>>,
    pub limit: i64,
}

/// `owner`'s records, newest first, with each one's attributions.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    query: TimelineQuery,
) -> QueryResult<Vec<(TimelineRecord, Vec<Attribution>)>> {
    let mut statement = timeline_records::table
        .filter(timeline_records::created_by.eq(query.owner))
        .select(TimelineRecord::as_select())
        .order((
            timeline_records::occurred_at.desc(),
            timeline_records::id.desc(),
        ))
        .limit(query.limit)
        .into_boxed();
    if let Some(project_id) = query.project_id {
        statement = statement.filter(
            timeline_records::id.eq_any(
                timeline_record_projects::table
                    .filter(timeline_record_projects::project_id.eq(project_id))
                    .select(timeline_record_projects::timeline_record_id),
            ),
        );
    }
    if let Some(before) = query.before {
        statement = statement.filter(timeline_records::occurred_at.lt(before));
    }
    let records: Vec<TimelineRecord> = statement.load(connection).await?;

    let ids: Vec<Uuid> = records.iter().map(|record| record.id).collect();
    let attributions: Vec<Attribution> = timeline_record_projects::table
        .filter(timeline_record_projects::timeline_record_id.eq_any(&ids))
        .select(Attribution::as_select())
        .load(connection)
        .await?;
    let mut by_record: HashMap<Uuid, Vec<Attribution>> = HashMap::new();
    for attribution in attributions {
        by_record
            .entry(attribution.timeline_record_id)
            .or_default()
            .push(attribution);
    }

    Ok(records
        .into_iter()
        .map(|record| {
            let attributions = by_record.remove(&record.id).unwrap_or_default();
            (record, attributions)
        })
        .collect())
}

/// The record `id` with its attributions, when `owner` owns it.
///
/// # Errors
/// [`diesel::result::Error::NotFound`] when it does not exist or belongs to someone else.
pub async fn find_owned(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<(TimelineRecord, Vec<Attribution>)> {
    let record = timeline_records::table
        .filter(timeline_records::id.eq(id))
        .filter(timeline_records::created_by.eq(owner))
        .select(TimelineRecord::as_select())
        .first(connection)
        .await?;
    let attributions = timeline_record_projects::table
        .filter(timeline_record_projects::timeline_record_id.eq(id))
        .select(Attribution::as_select())
        .load(connection)
        .await?;
    Ok((record, attributions))
}

/// Removes a record's attribution to a project; nothing to remove is not an error.
///
/// # Errors
/// Propagates any database error.
pub async fn unattribute(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
) -> QueryResult<()> {
    diesel::delete(
        timeline_record_projects::table
            .filter(timeline_record_projects::timeline_record_id.eq(record_id))
            .filter(timeline_record_projects::project_id.eq(project_id)),
    )
    .execute(connection)
    .await?;
    Ok(())
}

/// Attributes a record to a project, or changes how sure an existing attribution is.
///
/// # Errors
/// Propagates any database error, including a foreign key violation for an unknown record or
/// project.
pub async fn attribute(
    connection: &mut AsyncPgConnection,
    record_id: Uuid,
    project_id: Uuid,
    confidence: f32,
    attributed_by: &str,
) -> QueryResult<Attribution> {
    diesel::insert_into(timeline_record_projects::table)
        .values((
            timeline_record_projects::timeline_record_id.eq(record_id),
            timeline_record_projects::project_id.eq(project_id),
            timeline_record_projects::confidence.eq(confidence),
            timeline_record_projects::attributed_by.eq(attributed_by),
        ))
        .on_conflict((
            timeline_record_projects::timeline_record_id,
            timeline_record_projects::project_id,
        ))
        .do_update()
        .set((
            timeline_record_projects::confidence.eq(excluded(timeline_record_projects::confidence)),
            timeline_record_projects::attributed_by
                .eq(excluded(timeline_record_projects::attributed_by)),
        ))
        .returning(Attribution::as_returning())
        .get_result(connection)
        .await
}
