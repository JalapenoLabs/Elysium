// Copyright © 2026 Jalapeno Labs

//! `/api/v1/timeline`: the signed-in person's timeline, everything ingested for them, newest
//! first. See `docs/connectors.md`.

mod attribute_record;
mod list_timeline;

use axum::Router;
use axum::routing::{get, put};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use crate::models::timeline_record::{Attribution, TimelineRecord};
use crate::state::AppState;

pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_timeline::handle)).route(
        "/{id}/projects/{project_id}",
        put(attribute_record::attribute).delete(attribute_record::unattribute),
    )
}

/// A record as its owner sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimelineRecordResponse {
    id: Uuid,
    source_kind: String,
    connector_account_id: Option<Uuid>,
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
            connector_account_id: record.connector_account_id,
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
