// Copyright © 2026 Jalapeno Labs

//! `/api/v1/action-items`: the one list of what the user owes attention to.
//!
//! Every write is the user acting, recorded in the item's history as `user`. After a
//! write, the history it recorded, the item, and every initiative whose progress it
//! touches go out on the event stream. See `docs/action-items.md`.

mod add_project;
mod create_action_item;
mod create_comment;
mod delete_action_item;
mod delete_comment;
mod get_action_item;
mod join_initiative;
mod leave_initiative;
mod list_action_items;
mod list_comments;
mod list_history;
mod next_action_items;
mod remove_project;
mod restore_action_item;
mod snooze_action_item;
mod transition_action_item;
mod update_action_item;
mod update_comment;
mod wait_on;

use anyhow::Context;
use axum::Router;
use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::routing::{get, patch, post, put};
use chrono::{DateTime, Utc};
use diesel::result::{DatabaseErrorKind, Error as DieselError};
use diesel_async::AsyncPgConnection;
use serde::de::IntoDeserializer;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;
use validator::ValidationError;

use crate::action_items::{Transition, WorkError};
use crate::errors::ApiError;
use crate::models::action_item::{
    self, ActionItem, ActionItemPriority, ActionItemState, Memberships, Owner, ProjectFilter,
};
use crate::models::action_item_comment::Comment;
use crate::models::action_item_event::{HistoryEntry, Recorded};
use crate::models::initiative;
use crate::realtime::ServerEvent;
use crate::routes::v1::initiatives::initiative_responses;
use crate::state::AppState;

/// The transitions, by the path segment that asks for each.
const TRANSITION_ROUTES: [(&str, Transition); 4] = [
    ("/{id}/accept", Transition::Accept),
    ("/{id}/resolve", Transition::Resolve),
    ("/{id}/dismiss", Transition::Dismiss),
    ("/{id}/reopen", Transition::Reopen),
];

pub fn router() -> Router<AppState> {
    let mut router = Router::new()
        .route(
            "/",
            get(list_action_items::handle).post(create_action_item::handle),
        )
        .route("/next", get(next_action_items::handle))
        .route(
            "/{id}",
            get(get_action_item::handle)
                .patch(update_action_item::handle)
                .delete(delete_action_item::handle),
        )
        .route("/{id}/restore", post(restore_action_item::handle))
        .route("/{id}/snooze", post(snooze_action_item::handle))
        .route("/{id}/wait", post(wait_on::handle))
        .route("/{id}/history", get(list_history::handle))
        .route(
            "/{id}/comments",
            get(list_comments::handle).post(create_comment::handle),
        )
        .route(
            "/{id}/comments/{comment_id}",
            patch(update_comment::handle).delete(delete_comment::handle),
        )
        .route(
            "/{id}/projects/{project_id}",
            put(add_project::handle).delete(remove_project::handle),
        )
        .route(
            "/{id}/initiatives/{initiative_id}",
            put(join_initiative::handle).delete(leave_initiative::handle),
        );

    for (path, transition) in TRANSITION_ROUTES {
        router = router.route(
            path,
            post(
                move |state: State<AppState>, id: Result<Path<Uuid>, PathRejection>| {
                    transition_action_item::handle(state, id, transition)
                },
            ),
        );
    }
    router
}

/// An item as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionItemResponse {
    id: Uuid,
    title: String,
    notes: String,
    state: ActionItemState,
    priority: ActionItemPriority,
    due_at: Option<DateTime<Utc>>,
    snoozed_until: Option<DateTime<Utc>>,
    waiting_on: Option<String>,
    owner: Owner,
    resolved_at: Option<DateTime<Utc>>,
    dismissed_at: Option<DateTime<Utc>>,
    deleted_at: Option<DateTime<Utc>>,
    project_ids: Vec<Uuid>,
    /// The initiatives it is in now, leaving out deleted ones.
    initiative_ids: Vec<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl ActionItemResponse {
    pub fn new(item: ActionItem, memberships: &Memberships) -> Self {
        Self {
            owner: item.owner(),
            project_ids: memberships.project_ids(item.id),
            initiative_ids: memberships.initiative_ids(item.id),
            id: item.id,
            title: item.title,
            notes: item.notes,
            state: item.state,
            priority: item.priority,
            due_at: item.due_at,
            snoozed_until: item.snoozed_until,
            waiting_on: item.waiting_on,
            resolved_at: item.resolved_at,
            dismissed_at: item.dismissed_at,
            deleted_at: item.deleted_at,
            created_at: item.created_at,
            updated_at: item.updated_at,
        }
    }
}

/// A comment as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentResponse {
    id: Uuid,
    action_item_id: Uuid,
    /// `user`, `elysia`, `session:<number>`, or `watcher:<provider>`.
    author: String,
    body: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<Comment> for CommentResponse {
    fn from(comment: Comment) -> Self {
        Self {
            id: comment.id,
            action_item_id: comment.action_item_id,
            author: comment.author,
            body: comment.body,
            created_at: comment.created_at,
            updated_at: comment.updated_at,
        }
    }
}

/// A history entry as clients see it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryResponse {
    id: Uuid,
    /// Set for entries about an item, including its joining or leaving an initiative.
    action_item_id: Option<Uuid>,
    /// Set for entries about an initiative, including items joining or leaving it.
    initiative_id: Option<Uuid>,
    kind: String,
    /// `user`, `elysia`, `session:<number>`, or `watcher:<provider>`.
    actor: String,
    data: Value,
    created_at: DateTime<Utc>,
}

impl From<HistoryEntry> for HistoryEntryResponse {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            id: entry.id,
            action_item_id: entry.action_item_id,
            initiative_id: entry.initiative_id,
            kind: entry.kind,
            actor: entry.actor,
            data: entry.data,
            created_at: entry.created_at,
        }
    }
}

/// `items` as clients see them, each with its projects and initiatives.
///
/// # Errors
/// Propagates any database error.
pub async fn item_responses(
    connection: &mut AsyncPgConnection,
    items: Vec<ActionItem>,
) -> Result<Vec<ActionItemResponse>, ApiError> {
    let ids: Vec<Uuid> = items.iter().map(|item| item.id).collect();
    let memberships = action_item::memberships(connection, &ids).await?;
    Ok(items
        .into_iter()
        .map(|item| ActionItemResponse::new(item, &memberships))
        .collect())
}

/// Sends a write's history entries to every client.
pub fn publish_history(state: &AppState, history: Vec<HistoryEntry>) {
    for entry in history {
        state
            .events
            .publish(&ServerEvent::HistoryAppended(entry.into()));
    }
}

/// Publishes the initiatives with these ids that are not deleted, with their progress as
/// of `now`.
///
/// # Errors
/// Propagates any database error.
pub async fn publish_initiatives(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    initiative_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<(), ApiError> {
    let initiatives = initiative::find_live(connection, initiative_ids).await?;
    for response in initiative_responses(connection, initiatives, now).await? {
        state
            .events
            .publish(&ServerEvent::InitiativeUpserted(response));
    }
    Ok(())
}

/// After a write to an item: publishes the history it recorded, the item itself (as
/// deleted when it is), and every initiative whose progress it may have moved: those the
/// item is in, plus `left_initiative_ids` it just left. Answers the item as clients see it.
///
/// A write that changed nothing publishes nothing.
///
/// # Errors
/// Propagates any database error.
pub async fn publish_item_write(
    state: &AppState,
    connection: &mut AsyncPgConnection,
    written: Recorded<ActionItem>,
    left_initiative_ids: &[Uuid],
    now: DateTime<Utc>,
) -> Result<ActionItemResponse, ApiError> {
    let Recorded { record, history } = written;
    let id = record.id;
    let is_deleted = record.deleted_at.is_some();
    let response = item_responses(connection, vec![record])
        .await?
        .pop()
        .context("one item in, one response out")?;
    if history.is_empty() {
        return Ok(response);
    }

    publish_history(state, history);
    if is_deleted {
        state.events.publish(&ServerEvent::ActionItemDeleted { id });
    } else {
        state
            .events
            .publish(&ServerEvent::ActionItemUpserted(response.clone()));
    }

    let mut initiative_ids = action_item::current_initiative_ids(connection, id).await?;
    initiative_ids.extend_from_slice(left_initiative_ids);
    publish_initiatives(state, connection, &initiative_ids, now).await?;
    Ok(response)
}

/// A write that names a project that does not exist is the client's mistake.
pub fn refuse_unknown_projects(error: WorkError) -> ApiError {
    match error {
        WorkError::Database(DieselError::DatabaseError(
            DatabaseErrorKind::ForeignKeyViolation,
            _,
        )) => ApiError::BadRequest("projectIds lists a project that does not exist".to_owned()),
        other => other.into(),
    }
}

/// Rejects text that is only whitespace; `length` alone would accept `"   "`.
pub fn validate_not_blank(value: &str) -> Result<(), ValidationError> {
    if value.trim().is_empty() {
        return Err(ValidationError::new("blank").with_message("must not be blank".into()));
    }
    Ok(())
}

/// The longest name or address `waitingOn` and an owner's name hold, matching the
/// database: an email address is at most 320 characters.
const PERSON_MAX_CHARACTERS: usize = 320;

/// A name or address: 1 to 320 characters, not blank.
pub fn validate_person(value: &str) -> Result<(), ValidationError> {
    validate_not_blank(value)?;
    if value.chars().count() > PERSON_MAX_CHARACTERS {
        return Err(
            ValidationError::new("length").with_message("must be 320 characters or fewer".into())
        );
    }
    Ok(())
}

/// Someone else as an owner needs a name.
fn validate_owner(owner: &Owner) -> Result<(), ValidationError> {
    match owner {
        Owner::Other { name } => validate_person(name),
        Owner::User | Owner::Nobody => Ok(()),
    }
}

/// Parses a comma-separated query value, such as `state=inbox,open`, into a list. Empty
/// values are skipped, so `state=` filters on nothing.
///
/// # Errors
/// Fails naming the first value that does not parse.
pub fn comma_separated<'de, Source, Item>(deserializer: Source) -> Result<Vec<Item>, Source::Error>
where
    Source: Deserializer<'de>,
    Item: Deserialize<'de>,
{
    let text = String::deserialize(deserializer)?;
    text.split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| {
            Item::deserialize(value.into_deserializer())
                .map_err(|error: serde::de::value::Error| serde::de::Error::custom(error))
        })
        .collect()
}

/// Parses `project=none` or `project=<id>`.
///
/// # Errors
/// Fails for anything else.
pub fn project_filter<'de, Source>(
    deserializer: Source,
) -> Result<Option<ProjectFilter>, Source::Error>
where
    Source: Deserializer<'de>,
{
    let text = String::deserialize(deserializer)?;
    if text == "none" {
        return Ok(Some(ProjectFilter::Unassigned));
    }
    let project_id = Uuid::parse_str(&text)
        .map_err(|_parse_error| serde::de::Error::custom("project must be a project id or none"))?;
    Ok(Some(ProjectFilter::Project(project_id)))
}

/// Sorts ids and drops repeats, so each is stored once.
pub fn unique_ids(mut ids: Vec<Uuid>) -> Vec<Uuid> {
    ids.sort_unstable();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct Query {
        #[serde(default, deserialize_with = "comma_separated")]
        state: Vec<ActionItemState>,
        #[serde(default, deserialize_with = "project_filter")]
        project: Option<ProjectFilter>,
    }

    fn query(text: &str) -> Result<Query, axum::extract::rejection::QueryRejection> {
        let uri: axum::http::Uri = format!("/?{text}").parse().expect("a valid uri");
        axum::extract::Query::try_from_uri(&uri).map(|axum::extract::Query(query)| query)
    }

    #[test]
    fn states_arrive_comma_separated_and_unknown_ones_are_refused() {
        let parsed = query("state=inbox,open").expect("parses");
        assert_eq!(
            parsed.state,
            [ActionItemState::Inbox, ActionItemState::Open]
        );
        assert!(query("").expect("parses").state.is_empty());
        assert!(query("state=").expect("parses").state.is_empty());
        query("state=inbox,archived").expect_err("archived is not a state");
    }

    #[test]
    fn project_is_an_id_or_none() {
        let id = Uuid::now_v7();
        assert_eq!(
            query(&format!("project={id}")).expect("parses").project,
            Some(ProjectFilter::Project(id))
        );
        assert_eq!(
            query("project=none").expect("parses").project,
            Some(ProjectFilter::Unassigned)
        );
        query("project=elysium").expect_err("a name is not an id");
    }

    #[test]
    fn owners_are_the_user_nobody_or_someone_named() {
        let other: Owner =
            serde_json::from_value(json!({ "kind": "other", "name": "sam@example.com" }))
                .expect("parses");
        validate_owner(&other).expect("named");

        let blank: Owner =
            serde_json::from_value(json!({ "kind": "other", "name": " " })).expect("parses");
        validate_owner(&blank).expect_err("blank names are refused");

        serde_json::from_value::<Owner>(json!({ "kind": "other" }))
            .expect_err("a name is required");
        serde_json::from_value::<Owner>(json!({ "kind": "someone" }))
            .expect_err("an unknown kind is refused");
        assert_eq!(
            serde_json::to_value(Owner::Nobody).expect("serializes"),
            json!({ "kind": "nobody" })
        );
    }

    #[test]
    fn ids_are_sorted_and_stored_once() {
        let first = Uuid::now_v7();
        let second = Uuid::now_v7();
        assert_eq!(unique_ids(vec![second, first, second]), [first, second]);
    }
}
