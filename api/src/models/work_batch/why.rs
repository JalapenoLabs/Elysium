// Copyright © 2026 Jalapeno Labs

//! What called for a batch, as a person deciding on it reads it: the timeline records its Project
//! Manager named as its cause, the messages sent to it that did, the action items it works on, each
//! by name, in the order the Project Manager named them, and the batches it was planned from
//! (`super::lineage`).
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

use super::{BatchState, WorkBatch};
use crate::database::schema::{
    action_items, knowledge_documents, project_manager_wakes, timeline_records, users, work_batches,
};
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

/// A message a person or Elysia sent the batch's Project Manager that called for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub id: Uuid,
    pub author_id: Uuid,
    /// Its author's name.
    pub author: String,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

/// An action item a batch works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: Uuid,
    pub title: String,
    pub state: ActionItemState,
}

/// A batch the batch was planned from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parent {
    pub id: Uuid,
    pub title: String,
    pub state: BatchState,
    pub generation: i32,
}

/// Why a batch exists.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Why {
    pub causes: Vec<Cause>,
    pub messages: Vec<Message>,
    pub items: Vec<Item>,
    pub parents: Vec<Parent>,
}

/// Reads why `batch` exists.
///
/// # Errors
/// Propagates any database error.
pub async fn read(connection: &mut AsyncPgConnection, batch: &WorkBatch) -> QueryResult<Why> {
    let record_ids = batch.source_records();
    let message_ids = batch.source_messages();
    let item_ids = batch.action_items();
    let parent_ids = batch.parents();

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
        .map(
            |(record_id, source_kind, title, occurred_at, document_id)| {
                let cause = Cause {
                    record_id,
                    source_kind,
                    title,
                    occurred_at,
                    document_id,
                };
                (record_id, cause)
            },
        )
        .collect();
    let mut messages: HashMap<Uuid, Message> = project_manager_wakes::table
        .inner_join(users::table)
        .filter(project_manager_wakes::id.eq_any(&message_ids))
        .filter(project_manager_wakes::project_id.eq(batch.project_id))
        .select((
            project_manager_wakes::id,
            project_manager_wakes::created_by,
            users::name,
            project_manager_wakes::body,
            project_manager_wakes::created_at,
        ))
        .load::<(Uuid, Uuid, String, Option<String>, DateTime<Utc>)>(connection)
        .await?
        .into_iter()
        .map(|(id, author_id, author, body, sent_at)| {
            let message = Message {
                id,
                author_id,
                author,
                body: body.unwrap_or_default(),
                sent_at,
            };
            (id, message)
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
    let mut parents: HashMap<Uuid, Parent> = work_batches::table
        .filter(work_batches::id.eq_any(&parent_ids))
        .select((
            work_batches::id,
            work_batches::title,
            work_batches::state,
            work_batches::generation,
        ))
        .load::<(Uuid, String, BatchState, i32)>(connection)
        .await?
        .into_iter()
        .map(|(id, title, state, generation)| {
            let parent = Parent {
                id,
                title,
                state,
                generation,
            };
            (id, parent)
        })
        .collect();

    Ok(Why {
        causes: record_ids
            .iter()
            .filter_map(|id| causes.remove(id))
            .collect(),
        messages: message_ids
            .iter()
            .filter_map(|id| messages.remove(id))
            .collect(),
        items: item_ids.iter().filter_map(|id| items.remove(id)).collect(),
        parents: parent_ids
            .iter()
            .filter_map(|id| parents.remove(id))
            .collect(),
    })
}
