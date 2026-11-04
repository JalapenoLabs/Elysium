// Copyright © 2026 Jalapeno Labs

//! `/api/v1/timeline`: the signed-in person's timeline, everything ingested for them, newest
//! first. See `docs/connectors.md`.

mod attribute_record;
mod get_record;
mod list_timeline;
mod record_history;
mod stream_record_media;

use axum::Json;
use axum::Router;
use axum::routing::{get, post, put};
use chrono::{DateTime, Utc};
use diesel_async::AsyncPgConnection;
use serde::Serialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::errors::ApiError;
use crate::models::attribution::HistoryEntry;
use crate::models::timeline_record::{self, Attribution, TimelineRecord};
use crate::realtime::ServerEvent;
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_timeline::handle))
        .route("/{id}", get(get_record::handle))
        .route("/{id}/media", get(stream_record_media::handle))
        .route(
            "/{id}/projects/{project_id}",
            put(attribute_record::attribute).delete(attribute_record::unattribute),
        )
        .route("/{id}/history", get(record_history::list))
        .route("/{id}/history/{entry_id}/undo", post(record_history::undo))
}

/// The answer to a change of what record `id` is attributed to: the record as it reads now, and
/// the history entry the change recorded (`null` when nothing changed), for Undo. A change is
/// announced as `timelineRecord.changed`, so the owner's other pages follow it.
async fn answer_changed(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    id: Uuid,
    owner: Uuid,
    entry: Option<HistoryEntry>,
) -> Result<Json<Value>, ApiError> {
    if entry.is_some() {
        state.events.publish(&ServerEvent::TimelineRecordChanged {
            id,
            created_by: owner,
        });
    }
    let (record, attributions) = timeline_record::find_owned(connection, id, owner).await?;
    Ok(Json(json!({
        "record": TimelineRecordResponse::new(record, attributions),
        "entryId": entry.map(|entry| entry.id),
    })))
}

/// One change to a record's attributions, as its owner sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryResponse {
    id: Uuid,
    timeline_record_id: Uuid,
    project_id: Uuid,
    /// `attributed` or `unattributed`.
    kind: String,
    /// Who made the change: a person, or Elysia's machine user.
    created_by: Uuid,
    /// What the attribution held before and after, each `{ confidence, attributedBy }` or
    /// `null`.
    before: Value,
    after: Value,
    /// The entry this one undid, when it is an undo.
    undid: Option<Uuid>,
    created_at: DateTime<Utc>,
}

impl From<HistoryEntry> for HistoryEntryResponse {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            undid: entry.data["undid"]
                .as_str()
                .and_then(|undid| Uuid::parse_str(undid).ok()),
            before: entry.data["before"].clone(),
            after: entry.data["after"].clone(),
            id: entry.id,
            timeline_record_id: entry.timeline_record_id,
            project_id: entry.project_id,
            kind: entry.kind,
            created_by: entry.created_by,
            created_at: entry.created_at,
        }
    }
}

/// A record as its owner sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRecordResponse {
    id: Uuid,
    source_kind: String,
    connector_id: Option<Uuid>,
    external_id: Option<String>,
    occurred_at: DateTime<Utc>,
    actor: String,
    title: String,
    body: String,
    url: Option<String>,
    media: Option<Value>,
    metadata: Value,
    action_item_id: Option<Uuid>,
    projects: Vec<AttributionResponse>,
    created_by: Uuid,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AttributionResponse {
    project_id: Uuid,
    confidence: f32,
    attributed_by: String,
    created_at: DateTime<Utc>,
}

impl TimelineRecordResponse {
    pub fn new(record: TimelineRecord, attributions: Vec<Attribution>) -> Self {
        Self {
            id: record.id,
            source_kind: record.source_kind,
            connector_id: record.connector_id,
            external_id: record.external_id,
            occurred_at: record.occurred_at,
            actor: record.actor,
            title: record.title,
            body: record.body,
            url: record.url,
            media: record.media,
            metadata: record.metadata,
            action_item_id: record.action_item_id,
            projects: attributions
                .into_iter()
                .map(|attribution| AttributionResponse {
                    project_id: attribution.project_id,
                    confidence: attribution.confidence,
                    attributed_by: attribution.attributed_by,
                    created_at: attribution.created_at,
                })
                .collect(),
            created_by: record.created_by,
            created_at: record.created_at,
        }
    }
}
