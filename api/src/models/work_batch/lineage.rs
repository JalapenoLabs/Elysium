// Copyright © 2026 Jalapeno Labs

//! Where a batch comes from: its origin and its generation, worked out from its provenance as it
//! is submitted and never taken from what its Project Manager says. They are the structural brake
//! on work that files work. See `docs/engine.md`, Origins and generations.
//!
//! # Origin
//!
//! A batch is **human-rooted** when anything it names as its cause traces to a person, and
//! **found** when everything it names is an agent's ([`Root`], read by [`roots`]):
//!
//! - A timeline record is a person's when a person owns it. Everything ingested (mail, chat, a
//!   meeting's segments, an issue a reader recorded, a draft the person sent) is its person's;
//!   what Elysium's agents put on a timeline (a Project Manager's turns, batches, and writes,
//!   merged pull requests, turn summaries) is a machine's.
//! - A message to its Project Manager is a person's when a person wrote it. Elysia's are hers.
//! - An action item is a person's when a person created it, a provider's watcher brought it in (an
//!   issue assigned to the person, new mail), a person approved the changeset that created it, or
//!   a person accepted it from the inbox. An item an agent raised that nobody accepted, such as
//!   Elysia's from a conversation or one a Project Manager wrote directly, is an agent's reading of
//!   something: the messages it came from are what root it, so the Project Manager names those.
//!
//! A batch that names nothing has no origin and is refused ([`Refusal::NoOrigin`]).
//!
//! # Generation
//!
//! Batch B **led to** batch C when C's Project Manager planned C from B ([`parents`]): the turn
//! that submitted C was woken by B (its result, its worker's question, its rejection or
//! cancellation) or by a record B made (its merged pull request), or C names a record B made as its
//! cause. The turn's wakes lead to nothing when C answers a person's message that turn took, since
//! wakes are debounced together and a person's new words are not a batch's follow-up. C's
//! generation is the greatest, over the batches that led to it, of each one's generation
//! plus one when a worker worked on it ([`decide`]). A batch no worker touched (one a person
//! rejected, or one withdrawn) adds nothing: what replaces it stands where it stood, since nothing a
//! worker did called for it. A batch nothing led to is generation 0, and one past
//! [`MAX_GENERATION`] is refused ([`Refusal::PastLimit`]).
//!
//! A person accepting a finding re-roots the work: the batch it makes is human-rooted and
//! generation 0, whatever led to the finding ([`Lineage::accepted_finding`]).

use std::collections::{HashMap, HashSet};

use diesel::prelude::*;
use diesel_async::{AsyncPgConnection, RunQueryDsl};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::database::schema::{
    action_item_events, changesets, project_manager_wakes, timeline_records, users, work_batches,
};
use crate::models::project_manager::WakeKind;
use crate::models::timeline_record::SHARED_OWNERS;
use crate::models::user::UserRole;

/// The deepest a chain of batches may go: a person's request, then at most three batches each
/// planned from what the one before's worker did. Past that the work is no longer what anyone
/// asked for but what the work itself turned up, so a person decides whether it goes on, by
/// accepting it as a finding, which starts the chain again. Raising it lets agents run longer
/// unattended; nothing stored depends on it.
pub const MAX_GENERATION: i32 = 3;

/// Where a batch comes from.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, diesel_derive_enum::DbEnum, Serialize, Deserialize,
)]
#[ExistingTypePath = "crate::database::schema::sql_types::WorkBatchOrigin"]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// It traces to a person: their message, an issue assigned to them, a meeting, or a
    /// coworker's request.
    HumanRooted,
    /// It traces only to what an agent noticed.
    Found,
}

/// One thing a batch names as its cause, by whether it traces to a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    Person,
    Agent,
}

/// A batch that led to the one being submitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Parent {
    pub id: Uuid,
    pub generation: i32,
    /// Whether a worker worked on it, so what it did may have called for the next.
    pub worked_on: bool,
}

/// A batch's origin, generation, and the batches it was planned from, as it is stored. Only
/// [`decide`] and [`Lineage::accepted_finding`] make one, so a batch past the limit or with no
/// origin can never be stored.
#[derive(Debug, Clone, PartialEq, Eq, Insertable)]
#[diesel(table_name = work_batches)]
pub struct Lineage {
    origin: Origin,
    generation: i32,
    parent_batch_ids: Value,
}

impl Lineage {
    /// The batch a person makes by accepting a finding: human-rooted, since a person decided the
    /// work is wanted, and generation 0, since that decision starts the chain again. Only
    /// `crate::findings::accept` makes its batch with it.
    pub fn accepted_finding() -> Self {
        Self {
            origin: Origin::HumanRooted,
            generation: 0,
            parent_batch_ids: json!([]),
        }
    }

    pub const fn origin(&self) -> Origin {
        self.origin
    }

    pub const fn generation(&self) -> i32 {
        self.generation
    }
}

/// Why a batch has no lineage it may be stored with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// It names nothing that called for it.
    #[error("it names nothing that called for it")]
    NoOrigin,
    /// It would be deeper than [`MAX_GENERATION`]; `parents` are the batches that led to it.
    #[error("it would be generation {generation}, past {MAX_GENERATION}")]
    PastLimit { generation: i32, parents: Vec<Uuid> },
}

/// The lineage of a batch naming `roots` as its cause, planned from `parents`: human-rooted when
/// any root is a person's, found otherwise, one generation deeper than its deepest parent a worker
/// worked on.
///
/// # Errors
/// [`Refusal::NoOrigin`] when it names nothing; [`Refusal::PastLimit`] when it would be deeper than
/// [`MAX_GENERATION`].
pub fn decide(roots: &[Root], parents: &[Parent]) -> Result<Lineage, Refusal> {
    if roots.is_empty() {
        return Err(Refusal::NoOrigin);
    }
    let origin = if roots.contains(&Root::Person) {
        Origin::HumanRooted
    } else {
        Origin::Found
    };

    let mut generation = 0;
    let mut parent_ids = Vec::with_capacity(parents.len());
    for parent in parents {
        generation = generation.max(parent.generation + i32::from(parent.worked_on));
        parent_ids.push(parent.id);
    }
    parent_ids.sort_unstable();
    parent_ids.dedup();

    if generation > MAX_GENERATION {
        return Err(Refusal::PastLimit {
            generation,
            parents: parent_ids,
        });
    }
    Ok(Lineage {
        origin,
        generation,
        parent_batch_ids: json!(parent_ids),
    })
}

/// What a submission names as its cause: timeline records, action items, and messages to its
/// Project Manager (`project_manager_wakes` of kind message).
#[derive(Debug, Clone, Copy)]
pub struct Provenance<'a> {
    pub records: &'a [Uuid],
    pub action_items: &'a [Uuid],
    pub messages: &'a [Uuid],
}

/// Whether each thing `provenance` names traces to a person, one [`Root`] each. Something that
/// does not exist, or a message that is not one of `project_id`'s, names nothing; the caller has
/// already refused those.
///
/// # Errors
/// Propagates any database error.
pub async fn roots(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    provenance: Provenance<'_>,
) -> QueryResult<Vec<Root>> {
    let record_owners: Vec<Uuid> = timeline_records::table
        .filter(timeline_records::id.eq_any(provenance.records))
        .select(timeline_records::created_by)
        .load(connection)
        .await?;
    let message_authors: Vec<Uuid> = project_manager_wakes::table
        .filter(project_manager_wakes::id.eq_any(provenance.messages))
        .filter(project_manager_wakes::project_id.eq(project_id))
        .filter(project_manager_wakes::kind.eq(WakeKind::Message))
        .select(project_manager_wakes::created_by)
        .load(connection)
        .await?;
    let item_events = item_events(connection, provenance.action_items).await?;

    // Who wrote, owns, or decided each thing named, read at once to learn which are people.
    let mut users_named: Vec<Uuid> = Vec::new();
    users_named.extend(&record_owners);
    users_named.extend(&message_authors);
    users_named.extend(item_events.iter().filter_map(|event| event.decided_by));
    let people = people_among(connection, &users_named).await?;

    let root_of = |is_person: bool| {
        if is_person { Root::Person } else { Root::Agent }
    };
    let mut roots: Vec<Root> = record_owners
        .iter()
        .chain(&message_authors)
        .map(|user| root_of(people.contains(user)))
        .collect();

    // An item is a person's when any entry of its history shows one stands behind it.
    let mut items: HashMap<Uuid, bool> = HashMap::new();
    for event in &item_events {
        *items.entry(event.action_item_id).or_default() |= event.roots_in_a_person(&people);
    }
    roots.extend(items.into_values().map(root_of));
    Ok(roots)
}

/// An entry of an action item's history that may show a person stands behind it.
#[derive(Debug, Clone, Queryable)]
struct ItemEvent {
    action_item_id: Uuid,
    kind: String,
    actor: String,
    data: Value,
    /// Who decided the changeset that recorded it, when one did.
    decided_by: Option<Uuid>,
}

impl ItemEvent {
    /// Whether it shows a person stands behind its item, `people` being every person among those
    /// who decided a changeset: a person or a provider's watcher created it, a person approved the
    /// changeset that created it, or a person accepted it from the inbox. Actors are written as
    /// `crate::action_items::Actor` writes them; a bare `user` is a person's from before accounts.
    fn roots_in_a_person(&self, people: &HashSet<Uuid>) -> bool {
        let by_a_person = self.actor == "user" || self.actor.starts_with("user:");
        match self.kind.as_str() {
            "created" => match self.decided_by {
                Some(decider) => people.contains(&decider),
                None => by_a_person || self.actor.starts_with("watcher:"),
            },
            "state_changed" => {
                by_a_person
                    && self.data.get("from").and_then(Value::as_str) == Some("inbox")
                    && self.data.get("to").and_then(Value::as_str) == Some("open")
            }
            _ => false,
        }
    }
}

/// Every entry of `item_ids`' histories that may root them in a person, with who decided the
/// changeset that recorded it.
async fn item_events(
    connection: &mut AsyncPgConnection,
    item_ids: &[Uuid],
) -> QueryResult<Vec<ItemEvent>> {
    if item_ids.is_empty() {
        return Ok(Vec::new());
    }
    action_item_events::table
        .left_join(changesets::table)
        .filter(action_item_events::action_item_id.eq_any(item_ids))
        .filter(action_item_events::kind.eq_any(["created", "state_changed"]))
        .select((
            action_item_events::action_item_id.assume_not_null(),
            action_item_events::kind,
            action_item_events::actor,
            action_item_events::data,
            changesets::decided_by.nullable(),
        ))
        .load(connection)
        .await
}

/// Whether any of `message_ids` is a person's message that turn `turn_id` took.
async fn asked_this_turn(
    connection: &mut AsyncPgConnection,
    turn_id: Uuid,
    message_ids: &[Uuid],
) -> QueryResult<bool> {
    if message_ids.is_empty() {
        return Ok(false);
    }
    let authors: Vec<Uuid> = project_manager_wakes::table
        .filter(project_manager_wakes::id.eq_any(message_ids))
        .filter(project_manager_wakes::turn_id.eq(turn_id))
        .filter(project_manager_wakes::kind.eq(WakeKind::Message))
        .select(project_manager_wakes::created_by)
        .load(connection)
        .await?;
    Ok(!people_among(connection, &authors).await?.is_empty())
}

/// The people among `user_ids`: those holding a person's role, not a machine's.
async fn people_among(
    connection: &mut AsyncPgConnection,
    user_ids: &[Uuid],
) -> QueryResult<HashSet<Uuid>> {
    if user_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let people: Vec<Uuid> = users::table
        .filter(users::id.eq_any(user_ids))
        .filter(users::role.eq_any([UserRole::Admin, UserRole::Member, UserRole::Guest]))
        .select(users::id)
        .load(connection)
        .await?;
    Ok(people.into_iter().collect())
}

/// The batches of `project_id` that led to one submitted from turn `turn_id` naming `provenance`:
/// those that woke the turn, by their own wake or a record they made, and those that made a record
/// it names. A record a batch made is one of Elysium's own (a Project Manager's record of the
/// batch, its merged pull request's), naming the batch in its metadata.
///
/// A batch that names a person's message this very turn took answers that message, which is new
/// words from a person, so what else woke the turn led to nothing: wakes are debounced into one
/// turn, and a batch's result arriving beside a person's request must not hold the request to the
/// batch's chain. A Project Manager cannot make such a message; records it names still count.
///
/// # Errors
/// Propagates any database error.
pub async fn parents(
    connection: &mut AsyncPgConnection,
    project_id: Uuid,
    turn_id: Option<Uuid>,
    provenance: Provenance<'_>,
) -> QueryResult<Vec<Parent>> {
    let mut batch_ids: Vec<Uuid> = Vec::new();
    let mut records: Vec<Uuid> = provenance.records.to_vec();
    let answered = match turn_id {
        Some(turn_id) => asked_this_turn(connection, turn_id, provenance.messages).await?,
        None => false,
    };
    if let Some(turn_id) = turn_id.filter(|_turn| !answered) {
        let woken_by: Vec<(Option<Uuid>, Option<Uuid>)> = project_manager_wakes::table
            .filter(project_manager_wakes::turn_id.eq(turn_id))
            .filter(project_manager_wakes::project_id.eq(project_id))
            .select((
                project_manager_wakes::work_batch_id,
                project_manager_wakes::timeline_record_id,
            ))
            .load(connection)
            .await?;
        for (batch, record) in woken_by {
            batch_ids.extend(batch);
            records.extend(record);
        }
    }

    if !records.is_empty() {
        let metadata: Vec<Value> = timeline_records::table
            .filter(timeline_records::id.eq_any(&records))
            .filter(timeline_records::created_by.eq_any(SHARED_OWNERS))
            .select(timeline_records::metadata)
            .load(connection)
            .await?;
        batch_ids.extend(metadata.iter().filter_map(|metadata| {
            metadata
                .get("batchId")
                .and_then(Value::as_str)
                .and_then(|id| id.parse::<Uuid>().ok())
        }));
    }
    if batch_ids.is_empty() {
        return Ok(Vec::new());
    }

    let parents = work_batches::table
        .filter(work_batches::id.eq_any(&batch_ids))
        .filter(work_batches::project_id.eq(project_id))
        .select((
            work_batches::id,
            work_batches::generation,
            work_batches::started_at.is_not_null(),
        ))
        .load::<(Uuid, i32, bool)>(connection)
        .await?
        .into_iter()
        .map(|(id, generation, worked_on)| Parent {
            id,
            generation,
            worked_on,
        })
        .collect();
    Ok(parents)
}

#[cfg(test)]
mod tests;
