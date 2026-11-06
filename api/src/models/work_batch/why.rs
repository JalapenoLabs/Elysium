// Copyright © 2026 Jalapeno Labs

//! What called for a batch, as a person deciding on it reads it: the timeline records its Project
//! Manager named as its cause, and the action items it works on, each by name, in the order the
//! Project Manager named them.
//!
//! A record is read by its title and kind with the knowledge base's document for it, which is
//! where any member opens what was ingested (`docs/knowledge.md`, Who sees what); its owner's
//! timeline answers no one else. Something named that is gone since, a record removed with its
//! connector or a deleted item, is left out.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use super::WorkBatch;
use crate::database::schema::{action_items, knowledge_documents, timeline_records};
use crate::models::action_item::ActionItemState;

/// A timeline record that called for a batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cause {
    pub record_id: Uuid,
    /// Where it came from, such as `slack` or `meeting_segment`.
    pub source_kind: String,
    pub title: String,
    pub occurred_at: DateTime<Utc>,
    /// Its document in the knowledge base, once it is indexed.
    pub document_id: Option<Uuid>,
}

/// An action item a batch works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: Uuid,
    pub title: String,
    pub state: ActionItemState,
}

/// Why a batch exists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Why {
    pub causes: Vec<Cause>,
    pub items: Vec<Item>,
}

/// Reads why `batch` exists.
///
/// # Errors
/// Propagates any database error.
pub async fn read(connection: &mut AsyncPgConnection, batch: &WorkBatch) -> QueryResult<Why> {
    let record_ids = batch.source_records();
    let item_ids = batch.action_items();

    let mut causes: HashMap<Uuid, Cause> = timeline_records::table
        .left_join(knowledge_documents::table)
        .filter(timeline_records::id.eq_any(&record_ids))
        .select((
            timeline_records::id,
            timeline_records::source_kind,
            timeline_records::title,
            timeline_records::occurred_at,
            knowledge_documents::id.nullable(),
        ))
        .load::<(Uuid, String, String, DateTime<Utc>, Option<Uuid>)>(connection)
        .await?
        .into_iter()
        .map(|(record_id, source_kind, title, occurred_at, document_id)| {
            let cause = Cause {
                record_id,
                source_kind,
                title,
                occurred_at,
                document_id,
            };
            (record_id, cause)
        })
        .collect();
    let mut items: HashMap<Uuid, Item> = action_items::table
        .filter(action_items::id.eq_any(&item_ids))
        .filter(action_items::deleted_at.is_null())
        .select((action_items::id, action_items::title, action_items::state))
        .load::<(Uuid, String, ActionItemState)>(connection)
        .await?
        .into_iter()
        .map(|(id, title, state)| (id, Item { id, title, state }))
        .collect();

    Ok(Why {
        causes: record_ids
            .iter()
            .filter_map(|id| causes.remove(id))
            .collect(),
        items: item_ids.iter().filter_map(|id| items.remove(id)).collect(),
    })
}
