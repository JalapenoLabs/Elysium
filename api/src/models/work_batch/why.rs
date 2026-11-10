// Copyright © 2026 Jalapeno Labs

//! Why a batch exists, as one chain a person follows from what asked for it to the pull requests
//! that answer it ([`Link`]), and the batches it was planned from. See `docs/autonomy.md`,
//! Staying aware.
//!
//! The chain is read from the batch's provenance, the same that gives it its origin and
//! generation (`super::lineage`), never from a copy of its own: the records and messages its
//! Project Manager named, the action items it works on with the issues linked to each and the
//! records that raised each, then the batch, then the pull requests its worker recorded. In order:
//!
//! 1. the finding a person accepted, when the batch was made by accepting one;
//! 2. the messages to its Project Manager it names;
//! 3. the timeline records it names, a meeting segment's quote among them;
//! 4. for each item it names, the issues linked to the item, primary first, then the records
//!    that raised the item and are not named already;
//! 5. the items;
//! 6. the batch;
//! 7. its pull requests, in the order its worker recorded them.
//!
//! A record is opened by the knowledge base's document for it, which is where any member reads
//! what was ingested (`docs/knowledge.md`, Who sees what); its owner's timeline answers no one
//! else, and a meeting segment's document is the meeting at that topic. Something named that is
//! gone since, a record removed with its connector or a deleted item, is left out.
//!
//! [`read_many`] reads any number of batches' chains in one pass of queries, so the work queue
//! shows each row's head ([`Why::head`]) without a query per row.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

use super::pull_request::PullRequestState;
use super::{BatchState, WorkBatch};
use crate::database::schema::{
    action_item_links, action_items, findings, knowledge_documents, project_manager_wakes,
    timeline_records, users, work_batch_pull_requests, work_batches,
};
use crate::models::action_item::ActionItemState;
use crate::models::action_item_link::{LinkKind, LinkProvider};
use crate::models::finding::FindingState;

/// The most of a record's text a chain quotes. Enough for a few lines of a meeting's transcript
/// or the opening of a mail, which is what tells a person why; the whole of it is a click away,
/// in its knowledge base document.
pub const EXCERPT_MAX_CHARACTERS: usize = 600;

/// One link of a batch's chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Link {
    /// What a worker or Project Manager noticed, which a person accepted as work.
    Finding(Finding),
    /// A message a person or Elysia sent its Project Manager.
    Message(Message),
    /// A timeline record: a meeting segment, a mail, a chat message.
    Record(Record),
    /// An issue linked to one of its action items, in its tracker.
    Issue(Issue),
    /// An action item it works on.
    Item(Item),
    /// The batch itself.
    Batch(Batch),
    /// A pull request its worker opened.
    PullRequest(PullRequest),
}

/// A finding a person accepted, which made the batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: Uuid,
    /// What was seen, as it was reported.
    pub title: String,
    pub accepted_by_id: Uuid,
    /// Who accepted it, by name.
    pub accepted_by: String,
    pub accepted_at: DateTime<Utc>,
}

/// A message to the batch's Project Manager that called for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: Uuid,
    pub author_id: Uuid,
    /// Its author's name.
    pub author: String,
    pub body: String,
    pub sent_at: DateTime<Utc>,
}

/// A timeline record in a batch's chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub id: Uuid,
    /// Where it came from, such as `slack` or `meeting_segment`.
    pub source_kind: String,
    pub title: String,
    pub occurred_at: DateTime<Utc>,
    /// What it said, its first [`EXCERPT_MAX_CHARACTERS`]: a meeting segment's quote.
    pub excerpt: String,
    /// The meeting a segment is a topic of, by its title.
    pub meeting_title: Option<String>,
    /// Its document in the knowledge base, once it is indexed.
    pub document_id: Option<Uuid>,
}

/// An issue linked to an action item a batch works on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// The item it is linked to.
    pub action_item_id: Uuid,
    pub provider: LinkProvider,
    /// `ELY-12`, `owner/name#12`, or `JL-12`.
    pub key: String,
    pub url: String,
    /// Its title as the link last read it.
    pub title: String,
}

/// An action item a batch works on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: Uuid,
    pub title: String,
    pub state: ActionItemState,
}

/// The batch whose chain it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: Uuid,
    pub project_id: Uuid,
    pub title: String,
}

/// A pull request a batch's worker recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequest {
    pub id: Uuid,
    /// `owner/name`.
    pub repository: String,
    pub number: i32,
    pub url: String,
    pub title: String,
    pub state: PullRequestState,
}

/// A batch the batch was planned from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Parent {
    pub id: Uuid,
    pub title: String,
    pub state: BatchState,
    pub generation: i32,
}

/// Why a batch exists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Why {
    /// From what asked for it to its pull requests, in the module's order.
    pub chain: Vec<Link>,
    /// The batches it was planned from, in the order it keeps them.
    pub parents: Vec<Parent>,
}

impl Why {
    /// The batch whose chain it is.
    ///
    /// # Panics
    /// Panics for a chain without its batch, which [`read_many`] never makes.
    pub fn batch(&self) -> &Batch {
        self.chain
            .iter()
            .find_map(|link| match link {
                Link::Batch(batch) => Some(batch),
                _other => None,
            })
            .expect("every chain holds its batch")
    }

    /// What the chain starts from: the first thing that asked for the batch, or the first item
    /// it works on, or `None` for a batch that names nothing.
    pub fn head(&self) -> Option<&Link> {
        self.chain
            .iter()
            .find(|link| !matches!(link, Link::Batch(_) | Link::PullRequest(_)))
    }
}

/// Reads why `batch` exists.
///
/// # Errors
/// Propagates any database error.
pub async fn read(connection: &mut AsyncPgConnection, batch: &WorkBatch) -> QueryResult<Why> {
    let mut read = read_many(connection, &[batch]).await?;
    Ok(read
        .remove(&batch.id)
        .expect("read_many answers every batch it is given"))
}

/// Reads why each of `batches` exists, by id, in one query per kind of link however many there
/// are.
///
/// # Errors
/// Propagates any database error.
pub async fn read_many(
    connection: &mut AsyncPgConnection,
    batches: &[&WorkBatch],
) -> QueryResult<HashMap<Uuid, Why>> {
    let named = Named::of(batches);
    let items = items(connection, &named.items).await?;
    let issues = issues(connection, &named.items).await?;
    let records = records(connection, &named.records, &named.items).await?;
    let messages = messages(connection, &named.messages).await?;
    let parents = parents(connection, &named.parents).await?;
    let pull_requests = pull_requests(connection, &named.batches).await?;
    let findings = findings(connection, &named.batches).await?;

    let mut whys = HashMap::with_capacity(batches.len());
    for batch in batches {
        let item_ids = batch.action_items();
        let mut chain: Vec<Link> = Vec::new();
        if let Some(finding) = findings.get(&batch.id) {
            chain.push(Link::Finding(finding.clone()));
        }

        let mut listed: HashSet<Uuid> = HashSet::new();
        for message_id in batch.source_messages() {
            // A message is named only by a batch of its own project.
            let Some((project_id, message)) = messages.get(&message_id) else {
                continue;
            };
            if *project_id == batch.project_id {
                chain.push(Link::Message(message.clone()));
            }
        }
        for record_id in batch.source_records() {
            if let Some(found) = records.by_id.get(&record_id)
                && listed.insert(record_id)
            {
                chain.push(Link::Record(found.clone()));
            }
        }
        for item_id in &item_ids {
            if !items.contains_key(item_id) {
                continue;
            }
            for issue in issues.get(item_id).into_iter().flatten() {
                chain.push(Link::Issue(issue.clone()));
            }
            let raised = records.raised.get(item_id).into_iter().flatten();
            for record in raised {
                if listed.insert(record.id) {
                    chain.push(Link::Record(record.clone()));
                }
            }
        }
        for item_id in &item_ids {
            if let Some(item) = items.get(item_id) {
                chain.push(Link::Item(item.clone()));
            }
        }
        chain.push(Link::Batch(Batch {
            id: batch.id,
            project_id: batch.project_id,
            title: batch.title.clone(),
        }));
        for pull_request in pull_requests.get(&batch.id).into_iter().flatten() {
            chain.push(Link::PullRequest(pull_request.clone()));
        }

        let parents = batch
            .parents()
            .iter()
            .filter_map(|id| parents.get(id).cloned())
            .collect();
        whys.insert(batch.id, Why { chain, parents });
    }
    Ok(whys)
}

/// Everything a set of batches names, each id once.
#[derive(Default)]
struct Named {
    records: Vec<Uuid>,
    items: Vec<Uuid>,
    messages: Vec<Uuid>,
    parents: Vec<Uuid>,
    batches: Vec<Uuid>,
}

impl Named {
    fn of(batches: &[&WorkBatch]) -> Self {
        let mut named = Self::default();
        for batch in batches {
            named.records.extend(batch.source_records());
            named.items.extend(batch.action_items());
            named.messages.extend(batch.source_messages());
            named.parents.extend(batch.parents());
            named.batches.push(batch.id);
        }
        for ids in [
            &mut named.records,
            &mut named.items,
            &mut named.messages,
            &mut named.parents,
        ] {
            ids.sort_unstable();
            ids.dedup();
        }
        named
    }
}

/// The items among `item_ids` that are not deleted, by id.
async fn items(
    connection: &mut AsyncPgConnection,
    item_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Item>> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, String, ActionItemState)> = action_items::table
        .filter(action_items::id.eq_any(item_ids))
        .filter(action_items::deleted_at.is_null())
        .select((action_items::id, action_items::title, action_items::state))
        .load(connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, title, state)| (id, Item { id, title, state }))
        .collect())
}

/// The issues linked to each of `item_ids`, primary first, then in the order they were linked.
async fn issues(
    connection: &mut AsyncPgConnection,
    item_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<Issue>>> {
    if item_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, LinkProvider, String, String, String)> = action_item_links::table
        .filter(action_item_links::action_item_id.eq_any(item_ids))
        .filter(action_item_links::kind.eq(LinkKind::Issue))
        .order((
            action_item_links::is_primary.desc(),
            action_item_links::created_at.asc(),
            action_item_links::id.asc(),
        ))
        .select((
            action_item_links::action_item_id,
            action_item_links::provider,
            action_item_links::external_key,
            action_item_links::url,
            action_item_links::title,
        ))
        .load(connection)
        .await?;
    let mut issues: HashMap<Uuid, Vec<Issue>> = HashMap::new();
    for (action_item_id, provider, key, url, title) in rows {
        issues.entry(action_item_id).or_default().push(Issue {
            action_item_id,
            provider,
            key,
            url,
            title,
        });
    }
    Ok(issues)
}

/// The records batches name, and the records that raised the items they name.
struct Records {
    by_id: HashMap<Uuid, Record>,
    /// By the item each raised, oldest first.
    raised: HashMap<Uuid, Vec<Record>>,
}

/// Every record among `record_ids`, and every record that raised one of `item_ids`, in one read.
async fn records(
    connection: &mut AsyncPgConnection,
    record_ids: &[Uuid],
    item_ids: &[Uuid],
) -> QueryResult<Records> {
    let mut read = Records {
        by_id: HashMap::new(),
        raised: HashMap::new(),
    };
    if record_ids.is_empty() && item_ids.is_empty() {
        return Ok(read);
    }
    let rows: Vec<RecordRow> = timeline_records::table
        .left_join(knowledge_documents::table)
        .filter(
            timeline_records::id
                .eq_any(record_ids)
                .or(timeline_records::action_item_id.eq_any(item_ids)),
        )
        .order((
            timeline_records::occurred_at.asc(),
            timeline_records::id.asc(),
        ))
        .select((
            timeline_records::id,
            timeline_records::source_kind,
            timeline_records::title,
            timeline_records::occurred_at,
            timeline_records::body,
            timeline_records::metadata,
            timeline_records::action_item_id,
            knowledge_documents::id.nullable(),
        ))
        .load(connection)
        .await?;
    for row in rows {
        let (record, raised) = record_of(row);
        if let Some(item_id) = raised.filter(|item| item_ids.contains(item)) {
            read.raised.entry(item_id).or_default().push(record.clone());
        }
        read.by_id.insert(record.id, record);
    }
    Ok(read)
}

type RecordRow = (
    Uuid,
    String,
    String,
    DateTime<Utc>,
    String,
    Value,
    Option<Uuid>,
    Option<Uuid>,
);

/// A record as a chain shows it, and the item it raised.
fn record_of(row: RecordRow) -> (Record, Option<Uuid>) {
    let (id, source_kind, title, occurred_at, body, metadata, raised, document_id) = row;
    let body = body.trim();
    let mut excerpt: String = body.chars().take(EXCERPT_MAX_CHARACTERS).collect();
    // The excerpt is the body's start, so being shorter in bytes means it was cut.
    if excerpt.len() < body.len() {
        excerpt.push('…');
    }
    let meeting_title = metadata
        .get("meetingTitle")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let record = Record {
        id,
        source_kind,
        title,
        occurred_at,
        excerpt,
        meeting_title,
        document_id,
    };
    (record, raised)
}

/// A message as it is read: its id, its project, its author's id and name, what it said, and
/// when it was sent.
type MessageRow = (Uuid, Uuid, Uuid, String, Option<String>, DateTime<Utc>);

/// The messages among `message_ids`, by id, each with the project whose Project Manager it was
/// sent to.
async fn messages(
    connection: &mut AsyncPgConnection,
    message_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, (Uuid, Message)>> {
    if message_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<MessageRow> = project_manager_wakes::table
        .inner_join(users::table)
        .filter(project_manager_wakes::id.eq_any(message_ids))
        .select((
            project_manager_wakes::id,
            project_manager_wakes::project_id,
            project_manager_wakes::created_by,
            users::name,
            project_manager_wakes::body,
            project_manager_wakes::created_at,
        ))
        .load(connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(|(id, project_id, author_id, author, body, sent_at)| {
            let message = Message {
                id,
                author_id,
                author,
                body: body.unwrap_or_default(),
                sent_at,
            };
            (id, (project_id, message))
        })
        .collect())
}

/// The batches among `parent_ids`, by id.
async fn parents(
    connection: &mut AsyncPgConnection,
    parent_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Parent>> {
    if parent_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, String, BatchState, i32)> = work_batches::table
        .filter(work_batches::id.eq_any(parent_ids))
        .select((
            work_batches::id,
            work_batches::title,
            work_batches::state,
            work_batches::generation,
        ))
        .load(connection)
        .await?;
    Ok(rows
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
        .collect())
}

/// The finding whose acceptance made each of `batch_ids`, by the batch, with who accepted it.
async fn findings(
    connection: &mut AsyncPgConnection,
    batch_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Finding>> {
    if batch_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, Uuid, String, Uuid, String, DateTime<Utc>)> = findings::table
        // `findings` names two people, its reporter's machine user and its decider, so the join
        // says which.
        .inner_join(users::table.on(findings::decided_by.eq(users::id.nullable())))
        .filter(findings::accepted_batch_id.eq_any(batch_ids))
        .filter(findings::state.eq(FindingState::Accepted))
        .select((
            // Never null here: the filter above matched it.
            findings::accepted_batch_id.assume_not_null(),
            findings::id,
            findings::title,
            users::id,
            users::name,
            // Never null for a decided finding (`findings_decided_once_not_open`).
            findings::decided_at.assume_not_null(),
        ))
        .load(connection)
        .await?;
    Ok(rows
        .into_iter()
        .map(
            |(batch_id, id, title, accepted_by_id, accepted_by, accepted_at)| {
                let finding = Finding {
                    id,
                    title,
                    accepted_by_id,
                    accepted_by,
                    accepted_at,
                };
                (batch_id, finding)
            },
        )
        .collect())
}

/// The pull requests each of `batch_ids` recorded, in the order its worker recorded them.
async fn pull_requests(
    connection: &mut AsyncPgConnection,
    batch_ids: &[Uuid],
) -> QueryResult<HashMap<Uuid, Vec<PullRequest>>> {
    if batch_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, Uuid, String, i32, String, String, PullRequestState)> =
        work_batch_pull_requests::table
            .filter(work_batch_pull_requests::work_batch_id.eq_any(batch_ids))
            .order(work_batch_pull_requests::id.asc())
            .select((
                work_batch_pull_requests::work_batch_id,
                work_batch_pull_requests::id,
                work_batch_pull_requests::repository,
                work_batch_pull_requests::number,
                work_batch_pull_requests::url,
                work_batch_pull_requests::title,
                work_batch_pull_requests::state,
            ))
            .load(connection)
            .await?;
    let mut pull_requests: HashMap<Uuid, Vec<PullRequest>> = HashMap::new();
    for (batch_id, id, repository, number, url, title, state) in rows {
        pull_requests
            .entry(batch_id)
            .or_default()
            .push(PullRequest {
                id,
                repository,
                number,
                url,
                title,
                state,
            });
    }
    Ok(pull_requests)
}

#[cfg(test)]
mod tests;
