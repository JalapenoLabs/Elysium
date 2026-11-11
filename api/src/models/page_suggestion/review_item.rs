// Copyright © 2026 Jalapeno Labs

//! The one inbox item per page that asks a person to review the edits suggested to it.
//!
//! An agent may suggest several edits to one page, and the company doc's review suggests to every
//! section at once, so people are told once per page, never once per suggestion: an item titled
//! with how many wait and the page's title, kept up to date as suggestions arrive and are decided,
//! and resolved once none are left. Which item a page has is a row of `page_suggestion_items`. It
//! mirrors a project's approval item for its proposed batches
//! (`crate::models::work_batch::approval_item`).
//!
//! Every write that adds or settles a pending suggestion calls [`refresh`] in its own transaction,
//! so the item never disagrees with the suggestions it counts. A person who resolves or dismisses
//! the item by hand is done with it: deciding the suggestions that remain changes nothing, and the
//! next suggestion raises a new one.
//!
//! Elysium raises the item itself, whoever suggested: an item raised as a person would read as
//! theirs, accepted, rather than as one asking them. It names the page's title and the count, never
//! what a suggestion says, which stays on the page for the company's members.

use chrono::{DateTime, Utc};
use diesel::prelude::*;
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl};
use uuid::Uuid;

use super::SuggestionState;
use crate::action_items::{Actor, Transition, WorkError};
use crate::database::schema::{page_suggestion_items, page_suggestions};
use crate::models::action_item::{
    self, ActionItem, ActionItemChanges, ActionItemPriority, ActionItemState, NewActionItem, Owner,
};
use crate::models::action_item_event::Recorded;
use crate::models::page::{self, Page};

/// Serializes every refresh of one page's item, in whichever process makes it, so two writes at
/// once never both raise one. Hashed with the page's id.
const LOCK_PREFIX: &str = "page_suggestions.review_item:";

/// Why a page's item is refreshed: a suggestion was made, which may need an item raised, or one was
/// decided, which only ever updates or resolves the one there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    Suggested,
    Settled,
}

/// Brings `page`'s review item in line with how many suggestions wait on it, at `now`: raises one
/// when a suggestion was just made and none is open, retitles the open one when the count moved,
/// and resolves it once none are left. Call it in the transaction that changed a suggestion.
/// Answers the item it wrote, for the caller to announce once that transaction commits.
///
/// # Errors
/// Propagates any database error.
///
/// # Panics
/// When an item's own rules refuse a write to an item this function has just locked and found
/// live, which would be a programming error.
pub async fn refresh(
    connection: &mut AsyncPgConnection,
    page: &Page,
    cause: Cause,
    now: DateTime<Utc>,
) -> QueryResult<Option<Recorded<ActionItem>>> {
    diesel::sql_query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind::<diesel::sql_types::Text, _>(format!("{LOCK_PREFIX}{}", page.id))
        .execute(connection)
        .await?;
    let waiting: i64 = page_suggestions::table
        .filter(page_suggestions::page_id.eq(page.id))
        .filter(page_suggestions::state.eq(SuggestionState::Pending.as_str()))
        .count()
        .get_result(connection)
        .await?;
    let open = open_item(connection, page.id).await?;

    let written = match (open, waiting) {
        (None, 0) => return Ok(None),
        (None, _) if cause == Cause::Settled => return Ok(None),
        (Some(item), 0) => {
            action_item::transition(connection, item.id, Transition::Resolve, Actor::System, now)
                .await
        }
        (Some(item), _) => {
            let wording = new_item(&page.title, waiting);
            if item.title == wording.title {
                return Ok(None);
            }
            let changes = ActionItemChanges {
                title: Some(wording.title),
                notes: Some(wording.notes),
                ..ActionItemChanges::default()
            };
            action_item::update(connection, item.id, changes, Actor::System, now).await
        }
        (None, _) => {
            let raised = action_item::create(
                connection,
                new_item(&page.title, waiting),
                Actor::System,
                now,
            )
            .await
            .map_err(database_only)?;
            remember(connection, page.id, raised.record.id).await?;
            Ok(raised)
        }
    };
    written.map(Some).map_err(database_only)
}

/// Raises the item of every page with suggestions waiting that never had one, at `now`, each in its
/// own transaction: suggestions that became pending without a suggestion being made, such as the
/// company doc's proposals the migration carried over. A page whose person closed its item by hand
/// has a row, so it is left alone, and once raised, a page's item is kept by [`refresh`] alone, so
/// running this again raises nothing. The engine runs it as it starts. A page whose item cannot be
/// raised is logged and left for the next start. Answers the items it raised, for the caller to
/// announce.
///
/// # Errors
/// Propagates a database error reading which pages need one.
pub async fn reconcile(
    connection: &mut AsyncPgConnection,
    now: DateTime<Utc>,
) -> QueryResult<Vec<Recorded<ActionItem>>> {
    let never_told: Vec<Uuid> = page_suggestions::table
        .filter(page_suggestions::state.eq(SuggestionState::Pending.as_str()))
        .filter(diesel::dsl::not(page_suggestions::page_id.eq_any(
            page_suggestion_items::table.select(page_suggestion_items::page_id),
        )))
        .select(page_suggestions::page_id)
        .distinct()
        .load(connection)
        .await?;
    let mut raised = Vec::with_capacity(never_told.len());
    for page_id in never_told {
        let item = connection
            .transaction(async |connection| {
                let page = page::find(connection, page_id).await?;
                refresh(connection, &page, Cause::Suggested, now).await
            })
            .await;
        // One page that fails waits for the next start; the rest are still told.
        match item {
            Ok(item) => raised.extend(item),
            Err(error) => tracing::event!(
                name: "page_suggestion.review_item.reconcile_failure",
                tracing::Level::WARN,
                page.id = %page_id,
                error.message = %error,
                "could not raise a page's review item; the next start tries again",
            ),
        }
    }
    Ok(raised)
}

/// `page_id`'s item while it is still open to act on: in the inbox or accepted, and not deleted,
/// locked until the transaction ends. `None` when it has none, or its person dealt with the one it
/// had.
async fn open_item(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
) -> QueryResult<Option<ActionItem>> {
    let item_id: Option<Uuid> = page_suggestion_items::table
        .find(page_id)
        .select(page_suggestion_items::action_item_id)
        .first(connection)
        .await
        .optional()?;
    let Some(item_id) = item_id else {
        return Ok(None);
    };
    match action_item::lock_live(connection, item_id).await {
        Ok(item) if matches!(item.state, ActionItemState::Inbox | ActionItemState::Open) => {
            Ok(Some(item))
        }
        // Resolved, dismissed, or deleted by its person: theirs to have closed.
        Ok(_) | Err(WorkError::Conflict(_)) => Ok(None),
        Err(error) => Err(database_only(error)),
    }
}

/// Records `item_id` as `page_id`'s item, replacing the one its person closed.
async fn remember(
    connection: &mut AsyncPgConnection,
    page_id: Uuid,
    item_id: Uuid,
) -> QueryResult<()> {
    diesel::insert_into(page_suggestion_items::table)
        .values((
            page_suggestion_items::page_id.eq(page_id),
            page_suggestion_items::action_item_id.eq(item_id),
            page_suggestion_items::created_by.eq(Actor::System.user_id()),
        ))
        .on_conflict(page_suggestion_items::page_id)
        .do_update()
        .set(page_suggestion_items::action_item_id.eq(item_id))
        .execute(connection)
        .await
        .map(drop)
}

/// The item for `waiting` suggestions to the page titled `title`, whose words an open item is kept
/// to as well.
fn new_item(title: &str, waiting: i64) -> NewActionItem {
    NewActionItem {
        title: item_title(title, waiting),
        notes: format!(
            "Agents suggested edits to the knowledge base page {title}. Nothing changes until a \
             person accepts them: accept or dismiss each on the page's Suggestions tab."
        ),
        state: ActionItemState::Inbox,
        priority: ActionItemPriority::Normal,
        due_at: None,
        owner: Owner::User,
        project_ids: Vec::new(),
        initiative_ids: Vec::new(),
    }
}

/// What the item says: how many suggestions wait on the page titled `title`.
fn item_title(title: &str, waiting: i64) -> String {
    if waiting == 1 {
        return format!("Review a suggested edit to {title}");
    }
    format!("Review {waiting} suggested edits to {title}")
}

/// The database error inside `error`. The item's own rules cannot refuse what this module writes:
/// it locks the item and checks it is open before updating or resolving it, and raises one with no
/// project or initiative.
fn database_only(error: WorkError) -> diesel::result::Error {
    match error {
        WorkError::Database(error) => error,
        refused => {
            panic!("a page's review item's write was refused by the item's rules: {refused}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_counts_what_waits_on_the_page() {
        assert_eq!(
            item_title("Glossary", 1),
            "Review a suggested edit to Glossary"
        );
        assert_eq!(
            item_title("Glossary", 3),
            "Review 3 suggested edits to Glossary"
        );
    }
}
