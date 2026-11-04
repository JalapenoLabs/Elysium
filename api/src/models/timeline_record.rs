// Copyright © 2026 Jalapeno Labs

//! The timeline: everything Elysium ingests, as one kind of record, attributed to projects.
//! See `docs/connectors.md`.
//!
//! A record belongs to whoever it was ingested for, and only they list it: a person's mail is
//! theirs even when it is about a shared project.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::pg::Pg;
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
    pub connector_id: Option<Uuid>,
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
    pub connector_id: Option<Uuid>,
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
            timeline_records::connector_id,
            timeline_records::external_id,
        ))
        .do_nothing()
        .returning(TimelineRecord::as_returning())
        .get_result(connection)
        .await
        .optional()
}

/// The record `connector_id` keeps under `external_id`, if it has one.
///
/// # Errors
/// Propagates any database error.
pub async fn find_by_external_id(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
    external_id: &str,
) -> QueryResult<Option<TimelineRecord>> {
    timeline_records::table
        .filter(timeline_records::connector_id.eq(connector_id))
        .filter(timeline_records::external_id.eq(external_id))
        .select(TimelineRecord::as_select())
        .first(connection)
        .await
        .optional()
}

/// The record `id`, when `owner` owns it, without its attributions.
///
/// # Errors
/// Propagates any database error.
pub async fn find_owned_record(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
) -> QueryResult<Option<TimelineRecord>> {
    timeline_records::table
        .filter(timeline_records::id.eq(id))
        .filter(timeline_records::created_by.eq(owner))
        .select(TimelineRecord::as_select())
        .first(connection)
        .await
        .optional()
}

/// Whether any record of `connector_id`'s whose external id starts with `prefix` raised an
/// action item: for a kind that records each version of a thing, whether that thing already
/// raised one, so editing it again does not raise another.
///
/// # Errors
/// Propagates any database error.
pub async fn has_raised_item(
    connection: &mut AsyncPgConnection,
    connector_id: Uuid,
    prefix: &str,
) -> QueryResult<bool> {
    let escaped = prefix
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    diesel::select(diesel::dsl::exists(
        timeline_records::table
            .filter(timeline_records::connector_id.eq(connector_id))
            .filter(
                timeline_records::external_id
                    .like(format!("{escaped}%"))
                    .escape('\\'),
            )
            .filter(timeline_records::action_item_id.is_not_null()),
    ))
    .get_result(connection)
    .await
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
    /// Only the record that raised this action item.
    pub action_item_id: Option<Uuid>,
    /// Only records that happened before this, for the next page.
    pub before: Option<DateTime<Utc>>,
    pub limit: i64,
}

/// `owner`'s records that `query` narrows to, on every page: its project and action item, but
/// not its paging. A page and the total it is counted against share this one filter, so they
/// never disagree about what the timeline holds.
fn matching(query: &TimelineQuery) -> timeline_records::BoxedQuery<'static, Pg> {
    let mut statement = timeline_records::table
        .filter(timeline_records::created_by.eq(query.owner))
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
    if let Some(action_item_id) = query.action_item_id {
        statement = statement.filter(timeline_records::action_item_id.eq(action_item_id));
    }
    statement
}

/// How many records `query` matches across every page, ignoring `before` and `limit`.
///
/// # Errors
/// Propagates any database error.
pub async fn count(connection: &mut AsyncPgConnection, query: &TimelineQuery) -> QueryResult<i64> {
    matching(query).count().get_result(connection).await
}

/// `owner`'s records, newest first, with each one's attributions.
///
/// # Errors
/// Propagates any database error.
pub async fn list(
    connection: &mut AsyncPgConnection,
    query: TimelineQuery,
) -> QueryResult<Vec<(TimelineRecord, Vec<Attribution>)>> {
    let mut statement = matching(&query)
        .select(TimelineRecord::as_select())
        .order((
            timeline_records::occurred_at.desc(),
            timeline_records::id.desc(),
        ))
        .limit(query.limit);
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

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use serde_json::json;

    use super::*;
    use crate::models::project::{self, NewProject};
    use crate::test_support::{TEST_PERSON_ID, migrated_database};

    /// A record of the test person's that happened `minutes` after a fixed moment.
    fn record_at(minutes: i64) -> NewTimelineRecord {
        let base = DateTime::parse_from_rfc3339("2026-11-04T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc);
        NewTimelineRecord {
            id: Uuid::now_v7(),
            source_kind: "gmail".to_owned(),
            connector_id: None,
            external_id: None,
            occurred_at: base + TimeDelta::minutes(minutes),
            actor: "Sam".to_owned(),
            title: "Launch plan".to_owned(),
            body: String::new(),
            url: None,
            media: None,
            metadata: json!({}),
            created_by: TEST_PERSON_ID,
        }
    }

    #[tokio::test]
    #[ignore = "needs TEST_DATABASE_URL; run api/scripts/verify-migrations.sh"]
    async fn counts_every_page_a_project_timeline_holds() {
        let (_url, mut connection) = migrated_database().await;
        let launch = project::create(
            &mut connection,
            &NewProject {
                created_by: TEST_PERSON_ID,
                name: "Launch".to_owned(),
                description: String::new(),
            },
        )
        .await
        .expect("project");

        for minutes in [0, 10, 20] {
            let record = insert_new(&mut connection, record_at(minutes))
                .await
                .expect("stored")
                .expect("new");
            attribute(&mut connection, record.id, launch.id, 1.0, "elysia")
                .await
                .expect("attributed");
        }
        // About nothing in particular, so only the whole timeline counts it.
        insert_new(&mut connection, record_at(30))
            .await
            .expect("stored")
            .expect("new");

        let first_page = TimelineQuery {
            owner: TEST_PERSON_ID,
            project_id: Some(launch.id),
            action_item_id: None,
            before: None,
            limit: 2,
        };
        let page = list(&mut connection, first_page).await.expect("listed");
        assert_eq!(page.len(), 2, "a page holds no more than its limit");
        assert_eq!(
            count(&mut connection, &first_page).await.expect("counted"),
            3
        );

        // An older page counts the same total: paging narrows the page, never the total.
        let older_page = TimelineQuery {
            before: Some(page[1].0.occurred_at),
            ..first_page
        };
        let older = list(&mut connection, older_page).await.expect("listed");
        assert_eq!(older.len(), 1);
        assert_eq!(
            count(&mut connection, &older_page).await.expect("counted"),
            3
        );

        let everything = TimelineQuery {
            project_id: None,
            ..first_page
        };
        assert_eq!(
            count(&mut connection, &everything).await.expect("counted"),
            4
        );
    }
}
