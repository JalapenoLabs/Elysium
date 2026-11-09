// Copyright © 2026 Jalapeno Labs

//! What an action item is attributed to: the projects it is in, how sure whoever put it there
//! was, and the one that leads it. See `docs/action-items.md`, Projects and the lead.
//!
//! Most items join projects for certain, as whoever made them chose (`crate::models::action_item`
//! keeps those memberships and the lead with them). An issue that arrives assigned to its person
//! comes in no project, and an attribution step, the one that segments meetings and
//! conversations (`crate::meetings::segment`), guesses its projects with a confidence: Elysia's
//! reading, [`attribute`]. A person corrects either the projects or the lead, and undoes any change
//! to them from the item's history ([`undo`]).
//!
//! # Anyone else's word stands
//!
//! Elysia never overrides anyone else. A project that anyone but her put the item in or took it
//! out of, or that a changeset someone approved did, is theirs: her reading leaves it as it is,
//! either way. A lead that anyone but her moved is theirs too: she never moves it again, nor takes
//! the item off its project. Her
//! own earlier reading she replaces, so an item read twice keeps no guess the second reading
//! dropped.
//!
//! # Nothing starts
//!
//! Attributing an item wakes no Project Manager. An item is not a timeline record, so the rule
//! that wakes a project's Project Manager for a record newly on it (`crate::models::attribution`)
//! does not reach it; assigning the item to its lead project's Project Manager is what starts
//! planning.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use serde_json::Value;
use uuid::Uuid;

use crate::action_items::{Actor, WorkError};
use crate::database::schema::{action_item_events, action_item_projects};
use crate::meetings::segmentation::SegmentProject;
use crate::models::action_item::{self, ActionItem, ActionItemState, CERTAIN, Joining, LeadMove};
use crate::models::action_item_event::{HistoryEntry, HistoryKind, Recorded};
use crate::models::project;

/// Applies Elysia's reading of item `id`: it joins each project she guessed, most sure first, with
/// her confidence, leaves each project her earlier reading put it in that this one drops, and is
/// led by the project she is surest of among those it is in. Whatever anyone else decided stands
/// (the module's docs), and a project removed since she read the item is left out.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`] for a
/// deleted, resolved, or dismissed one, and any other database error.
pub async fn attribute(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    guesses: &[SegmentProject],
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    // Most sure first, so the first project the item joins, which leads it, is her surest.
    let mut ordered: Vec<&SegmentProject> = guesses.iter().collect();
    ordered.sort_by(|left, right| right.confidence.total_cmp(&left.confidence));
    let elysia = Actor::Elysia.to_string();

    connection
        .transaction(async move |connection| {
            let mut item = action_item::lock_live(connection, id).await?;
            // Read again under the lock: the item may have been finished while she read it.
            if matches!(
                item.state,
                ActionItemState::Resolved | ActionItemState::Dismissed
            ) {
                return Err(WorkError::Conflict(
                    "the item was resolved or dismissed, so it is not attributed",
                ));
            }
            let decided = decided_projects(connection, id).await?;
            // A lead anyone else chose is theirs, and so is its project: leaving it would pass the
            // lead on.
            let lead_is_theirs = lead_decided(connection, id).await?;
            let mut history: Vec<HistoryEntry> = Vec::new();

            let held = action_item::project_attributions(connection, id).await?;
            for membership in held {
                let hers = membership.attributed_by == elysia;
                let in_reading = guesses
                    .iter()
                    .any(|guess| guess.project_id == membership.project_id);
                let their_lead =
                    lead_is_theirs && item.lead_project_id == Some(membership.project_id);
                if !hers || in_reading || their_lead || decided.contains(&membership.project_id) {
                    continue;
                }
                let left = action_item::leave_project(
                    connection,
                    item,
                    membership.project_id,
                    Actor::Elysia,
                    None,
                    now,
                )
                .await?;
                item = left.record;
                history.extend(left.history);
            }

            for guess in &ordered {
                if decided.contains(&guess.project_id) {
                    continue;
                }
                // Share-locked, so the project cannot go while the item joins it.
                let exists = project::lock_shared(connection, guess.project_id)
                    .await
                    .optional()?
                    .is_some();
                if !exists {
                    continue;
                }
                let joining = Joining {
                    project_id: guess.project_id,
                    confidence: guess.confidence,
                    attributed_by: elysia.clone(),
                    actor: Actor::Elysia,
                    undid: None,
                };
                let joined = action_item::join_project(connection, item, joining, now).await?;
                item = joined.record;
                history.extend(joined.history);
            }

            // Nobody chose the lead, so it is the surest of the item's projects: a person's choice
            // before her guesses, and her surest guess among those.
            if !lead_is_theirs {
                let surest = action_item::project_attributions(connection, id)
                    .await?
                    .into_iter()
                    .next()
                    .map(|membership| membership.project_id);
                if surest.is_some() {
                    let led = action_item::move_lead(
                        connection,
                        &item,
                        surest,
                        Actor::Elysia,
                        LeadMove::Chosen,
                        now,
                    )
                    .await?;
                    item = led.record;
                    history.extend(led.history);
                }
            }

            Ok(Recorded {
                record: item,
                history,
            })
        })
        .await
}

/// Makes project `project_id`, one of item `id`'s projects, its lead, as `actor`. Making the lead
/// the lead again changes nothing.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item, [`WorkError::Conflict`] for a
/// deleted one, [`WorkError::Invalid`] for a project the item is not in, and any other database
/// error.
pub async fn set_lead(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = action_item::lock_live(connection, id).await?;
            if membership(connection, id, project_id).await?.is_none() {
                return Err(WorkError::Invalid(
                    "the lead must be one of the item's projects; add it to the project first",
                ));
            }
            Ok(action_item::move_lead(
                connection,
                &item,
                Some(project_id),
                actor,
                LeadMove::Chosen,
                now,
            )
            .await?)
        })
        .await
}

/// Undoes history entry `entry_id` of item `id` as `actor`: a project it joined leaves it, a
/// project it left joins it again as it was held, and a moved lead moves back. The undo is an entry
/// of its own naming the one it reversed (`undid`), so it can be undone in turn. Only the latest
/// change to that project, or to the lead, is undone, so a later change is never overwritten, even
/// one that put things back as the entry left them.
///
/// # Errors
/// Returns [`diesel::result::Error::NotFound`] for an unknown item or an entry that is not its,
/// [`WorkError::Conflict`] for a deleted item or one that changed since the entry,
/// [`WorkError::Invalid`] for an entry of another kind, and any other database error.
pub async fn undo(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    entry_id: Uuid,
    actor: Actor,
    now: DateTime<Utc>,
) -> Result<Recorded<ActionItem>, WorkError> {
    connection
        .transaction(async move |connection| {
            let item = action_item::lock_live(connection, id).await?;
            let entry: HistoryEntry = action_item_events::table
                .find(entry_id)
                .filter(action_item_events::action_item_id.eq(id))
                .select(HistoryEntry::as_select())
                .first(connection)
                .await?;
            let undid = Some(entry.id);
            let kind = entry.kind.as_str();

            if kind == HistoryKind::ProjectAdded.as_str() {
                let project_id = project_of(&entry.data)?;
                let latest = latest_change(connection, id, Some(project_id)).await?;
                let still_there = membership(connection, id, project_id).await?.is_some();
                if latest != Some(entry.id) || !still_there {
                    return Err(WorkError::Conflict(
                        "the item's place in that project changed since",
                    ));
                }
                return Ok(action_item::leave_project(
                    connection, item, project_id, actor, undid, now,
                )
                .await?);
            }

            if kind == HistoryKind::ProjectRemoved.as_str() {
                let project_id = project_of(&entry.data)?;
                let latest = latest_change(connection, id, Some(project_id)).await?;
                let still_out = membership(connection, id, project_id).await?.is_none();
                if latest != Some(entry.id) || !still_out {
                    return Err(WorkError::Conflict(
                        "the item's place in that project changed since",
                    ));
                }
                let exists = project::lock_shared(connection, project_id)
                    .await
                    .optional()?
                    .is_some();
                if !exists {
                    return Err(WorkError::Conflict("that project was deleted since"));
                }
                // Put back as it was held; an entry that did not say joins as the undoer's own.
                let confidence = serde_json::from_value::<f32>(entry.data["confidence"].clone())
                    .unwrap_or(CERTAIN);
                let attributed_by = entry.data["attributedBy"]
                    .as_str()
                    .map_or_else(|| actor.to_string(), str::to_owned);
                let joining = Joining {
                    project_id,
                    confidence,
                    attributed_by,
                    actor,
                    undid,
                };
                return Ok(action_item::join_project(connection, item, joining, now).await?);
            }

            if kind == HistoryKind::LeadProjectChanged.as_str() {
                let (from, to) = lead_change(&entry.data)?;
                let latest = latest_change(connection, id, None).await?;
                if latest != Some(entry.id) || item.lead_project_id != to {
                    return Err(WorkError::Conflict("the item's lead changed since"));
                }
                let Some(from) = from else {
                    return Err(WorkError::Conflict(
                        "an item in a project always has a lead; choose another one instead",
                    ));
                };
                if membership(connection, id, from).await?.is_none() {
                    return Err(WorkError::Conflict(
                        "the item is no longer in the project that led it; undo its removal first",
                    ));
                }
                return Ok(action_item::move_lead(
                    connection,
                    &item,
                    Some(from),
                    actor,
                    LeadMove::Undo(entry.id),
                    now,
                )
                .await?);
            }

            Err(WorkError::Invalid(
                "only a change to an item's projects or its lead is undone from its history",
            ))
        })
        .await
}

/// The projects anyone but Elysia decided item `id` is in or out of, or that a changeset someone
/// approved did: hers to leave alone.
async fn decided_projects(
    connection: &mut AsyncPgConnection,
    id: Uuid,
) -> QueryResult<HashSet<Uuid>> {
    let kinds = [
        HistoryKind::ProjectAdded.as_str(),
        HistoryKind::ProjectRemoved.as_str(),
    ];
    let named: Vec<Option<String>> = action_item_events::table
        .filter(action_item_events::action_item_id.eq(id))
        .filter(action_item_events::kind.eq_any(kinds))
        .filter(
            action_item_events::actor
                .ne(Actor::Elysia.to_string())
                .or(action_item_events::changeset_id.is_not_null()),
        )
        .select(
            action_item_events::data
                .retrieve_as_text("projectId")
                .nullable(),
        )
        .load(connection)
        .await?;
    Ok(named
        .iter()
        .flatten()
        .filter_map(|project_id| Uuid::parse_str(project_id).ok())
        .collect())
}

/// Whether anyone but Elysia chose item `id`'s lead, or a changeset someone approved did. A lead
/// that passed on as the item's projects changed was nobody's choice.
async fn lead_decided(connection: &mut AsyncPgConnection, id: Uuid) -> QueryResult<bool> {
    diesel::select(diesel::dsl::exists(
        action_item_events::table
            .filter(action_item_events::action_item_id.eq(id))
            .filter(action_item_events::kind.eq(HistoryKind::LeadProjectChanged.as_str()))
            .filter(diesel::dsl::not(
                action_item_events::data.has_key("passedOn"),
            ))
            .filter(
                action_item_events::actor
                    .ne(Actor::Elysia.to_string())
                    .or(action_item_events::changeset_id.is_not_null()),
            ),
    ))
    .get_result(connection)
    .await
}

/// Item `id`'s membership in project `project_id`, when it has one.
async fn membership(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project_id: Uuid,
) -> QueryResult<Option<action_item::ProjectAttribution>> {
    action_item_projects::table
        .filter(action_item_projects::action_item_id.eq(id))
        .filter(action_item_projects::project_id.eq(project_id))
        .select(action_item::ProjectAttribution::as_select())
        .first(connection)
        .await
        .optional()
}

/// The latest entry that changed item `id`'s membership in `project`, or with `None` its lead.
async fn latest_change(
    connection: &mut AsyncPgConnection,
    id: Uuid,
    project: Option<Uuid>,
) -> QueryResult<Option<Uuid>> {
    let mut query = action_item_events::table
        .filter(action_item_events::action_item_id.eq(id))
        .into_boxed();
    query = match project {
        Some(project_id) => query
            .filter(action_item_events::kind.eq_any([
                HistoryKind::ProjectAdded.as_str(),
                HistoryKind::ProjectRemoved.as_str(),
            ]))
            .filter(
                action_item_events::data
                    .retrieve_as_text("projectId")
                    .eq(project_id.to_string()),
            ),
        None => query.filter(action_item_events::kind.eq(HistoryKind::LeadProjectChanged.as_str())),
    };
    query
        .order(action_item_events::id.desc())
        .select(action_item_events::id)
        .first(connection)
        .await
        .optional()
}

/// The project a `project_added` or `project_removed` entry names.
fn project_of(data: &Value) -> Result<Uuid, WorkError> {
    data["projectId"]
        .as_str()
        .and_then(|project_id| Uuid::parse_str(project_id).ok())
        .ok_or(WorkError::Invalid("the entry names no project"))
}

/// The `{ from, to }` of a `lead_project_changed` entry.
fn lead_change(data: &Value) -> Result<(Option<Uuid>, Option<Uuid>), WorkError> {
    let read = |field: &str| -> Result<Option<Uuid>, WorkError> {
        match &data[field] {
            Value::Null => Ok(None),
            Value::String(project_id) => Uuid::parse_str(project_id)
                .map(Some)
                .map_err(|_unreadable| WorkError::Invalid("the entry names no project")),
            _ => Err(WorkError::Invalid("the entry names no project")),
        }
    };
    Ok((read("from")?, read("to")?))
}

#[cfg(test)]
mod tests;
