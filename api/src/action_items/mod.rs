// Copyright © 2026 Jalapeno Labs

//! The rules of action items and initiatives that hold whatever stores them.
//!
//! Action items are the one list of everything the user owes attention to; initiatives
//! group them toward a goal that ends. `docs/action-items.md` is the design. This module
//! holds the parts of it that are pure decisions, so they are tested without a database:
//!
//! - [`Actor`]: who made a change, as the history records it.
//! - [`Transition`]: which state changes an item allows.
//! - [`next`]: which items are in Next, and in what order.
//! - [`progress`]: an initiative's resolved and total, now and over time.
//! - [`session_context`]: what a coding session started from an item is told about it.
//! - [`links`]: the external things items and initiatives point at, behind one provider
//!   trait, with the rules for what a provider's change does to an item.
//! - [`watcher`]: the poller that keeps links current and lands the writes Elysium owes.
//! - [`changesets`]: the changes anyone but the user proposes, how they depend on each
//!   other, and how the user's decisions on them cascade.
//!
//! The queries live in `crate::models::action_item` and its siblings, and call into these.

pub mod changesets;
pub mod links;
pub mod next;
pub mod progress;
pub mod session_context;
pub mod watcher;

use std::collections::HashMap;
use std::fmt;

use uuid::Uuid;

use crate::models::action_item::ActionItemState;
use crate::models::action_item_link::LinkProvider;
use crate::models::user::{CODING_AGENT_USER_ID, ELYSIA_USER_ID, SYSTEM_USER_ID};

/// Who made a change. Every history entry and comment names one.
///
/// People act over HTTP, a coding session's agent through the `elysium_work` tools
/// (`crate::tools::work`), and the watcher as it records what a provider reports. Elysia
/// and a session's agent also act when a person applies a changeset they proposed
/// ([`changesets`]).
///
/// Written as `user:<id>`, `elysia`, `session:<n>`, or `watcher:<provider>`. Rows written
/// before accounts existed say `user` alone, meaning the one person there was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    /// The person with this user id.
    User(Uuid),
    /// Elysium's AI assistant.
    Elysia,
    /// The agent of the coding session with this number.
    Session(i64),
    /// The watcher, recording a change it read from this provider.
    Watcher(LinkProvider),
}

impl Actor {
    /// The user a row this actor creates names in `created_by`: the person, or the machine
    /// that stands for Elysia, every coding agent, or the watcher.
    pub const fn user_id(self) -> Uuid {
        match self {
            Self::User(id) => id,
            Self::Elysia => ELYSIA_USER_ID,
            Self::Session(_) => CODING_AGENT_USER_ID,
            Self::Watcher(_) => SYSTEM_USER_ID,
        }
    }
}

impl fmt::Display for Actor {
    #[expect(
        clippy::renamed_function_params,
        reason = "`f` is a shorthand name; the project spells names out"
    )]
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(id) => write!(formatter, "user:{id}"),
            Self::Elysia => formatter.write_str("elysia"),
            Self::Session(number) => write!(formatter, "session:{number}"),
            Self::Watcher(provider) => write!(formatter, "watcher:{}", provider.as_str()),
        }
    }
}

/// How a recorded actor reads to a person or an agent: a person's name for `user:<id>`, and
/// the recorded form for everyone else (`elysia`, `session:<n>`, `watcher:<provider>`, and
/// the `user` of rows written before accounts).
pub fn actor_label(actor: &str, names: &HashMap<Uuid, String>) -> String {
    actor
        .strip_prefix("user:")
        .and_then(|id| Uuid::parse_str(id).ok())
        .and_then(|id| names.get(&id))
        .map_or_else(|| actor.to_owned(), Clone::clone)
}

/// A change of an item's state that the user asks for by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Takes an item out of the inbox: the user intends to do it.
    Accept,
    Resolve,
    Dismiss,
    /// Brings a resolved or dismissed item back as `open`, since the user accepted it once.
    Reopen,
}

/// What a transition requires and produces.
struct TransitionRule {
    from: &'static [ActionItemState],
    to: ActionItemState,
    /// Why an item in any other state refuses it.
    refusal: &'static str,
}

/// Every transition's rule. Resolving and dismissing both work from the inbox as well, so
/// an item that arrived already done, or that is not worth accepting, takes one step.
const fn rule(transition: Transition) -> TransitionRule {
    match transition {
        Transition::Accept => TransitionRule {
            from: &[ActionItemState::Inbox],
            to: ActionItemState::Open,
            refusal: "only an item in the inbox can be accepted",
        },
        Transition::Resolve => TransitionRule {
            from: &[ActionItemState::Inbox, ActionItemState::Open],
            to: ActionItemState::Resolved,
            refusal: "only an item in the inbox or open can be resolved",
        },
        Transition::Dismiss => TransitionRule {
            from: &[ActionItemState::Inbox, ActionItemState::Open],
            to: ActionItemState::Dismissed,
            refusal: "only an item in the inbox or open can be dismissed",
        },
        Transition::Reopen => TransitionRule {
            from: &[ActionItemState::Resolved, ActionItemState::Dismissed],
            to: ActionItemState::Open,
            refusal: "only a resolved or dismissed item can be reopened",
        },
    }
}

impl Transition {
    /// The state an item in `current` moves to.
    ///
    /// # Errors
    /// Returns why the transition does not apply to an item in `current`.
    pub fn apply(self, current: ActionItemState) -> Result<ActionItemState, &'static str> {
        let rule = rule(self);
        if rule.from.contains(&current) {
            return Ok(rule.to);
        }
        Err(rule.refusal)
    }
}

/// Why a change to an action item or initiative was refused.
#[derive(Debug, thiserror::Error)]
pub enum WorkError {
    #[error(transparent)]
    Database(#[from] diesel::result::Error),
    /// The change does not apply to the record as it stands, such as resolving a
    /// dismissed item or editing a deleted one.
    #[error("{0}")]
    Conflict(&'static str),
    /// The request names something that cannot take part, such as a deleted initiative.
    #[error("{0}")]
    Invalid(&'static str),
    /// The request cannot be used as given, in words that name what is wrong, such as the
    /// operation of a proposed changeset that names a deleted item.
    #[error("{0}")]
    Refused(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    const EVERY_STATE: [ActionItemState; 4] = [
        ActionItemState::Inbox,
        ActionItemState::Open,
        ActionItemState::Resolved,
        ActionItemState::Dismissed,
    ];

    fn reachable(transition: Transition) -> Vec<(ActionItemState, ActionItemState)> {
        EVERY_STATE
            .into_iter()
            .filter_map(|from| Some((from, transition.apply(from).ok()?)))
            .collect()
    }

    #[test]
    fn accepting_moves_only_the_inbox_to_open() {
        assert_eq!(
            reachable(Transition::Accept),
            [(ActionItemState::Inbox, ActionItemState::Open)]
        );
        assert_eq!(
            Transition::Accept.apply(ActionItemState::Open),
            Err("only an item in the inbox can be accepted")
        );
    }

    #[test]
    fn resolving_and_dismissing_work_from_the_inbox_and_open() {
        assert_eq!(
            reachable(Transition::Resolve),
            [
                (ActionItemState::Inbox, ActionItemState::Resolved),
                (ActionItemState::Open, ActionItemState::Resolved),
            ]
        );
        assert_eq!(
            reachable(Transition::Dismiss),
            [
                (ActionItemState::Inbox, ActionItemState::Dismissed),
                (ActionItemState::Open, ActionItemState::Dismissed),
            ]
        );
        Transition::Resolve
            .apply(ActionItemState::Dismissed)
            .expect_err("a dismissed item is reopened, not resolved");
        Transition::Dismiss
            .apply(ActionItemState::Resolved)
            .expect_err("a resolved item is reopened, not dismissed");
    }

    #[test]
    fn reopening_returns_finished_items_to_open() {
        assert_eq!(
            reachable(Transition::Reopen),
            [
                (ActionItemState::Resolved, ActionItemState::Open),
                (ActionItemState::Dismissed, ActionItemState::Open),
            ]
        );
    }

    #[test]
    fn people_read_as_their_names_and_everyone_else_as_recorded() {
        let person = Uuid::now_v7();
        let names = HashMap::from([(person, "Sam Rivera".to_owned())]);
        assert_eq!(actor_label(&format!("user:{person}"), &names), "Sam Rivera");
        let stranger = format!("user:{}", Uuid::now_v7());
        assert_eq!(
            actor_label(&stranger, &names),
            stranger,
            "an unknown person keeps the recorded form"
        );
        assert_eq!(actor_label("session:4", &names), "session:4");
        assert_eq!(actor_label("user", &names), "user");
    }

    #[test]
    fn actors_are_recorded_in_the_form_the_database_checks() {
        let person = Uuid::parse_str("0199a3c4-5d6e-7f80-9a1b-2c3d4e5f6071").expect("a UUID");
        assert_eq!(
            Actor::User(person).to_string(),
            "user:0199a3c4-5d6e-7f80-9a1b-2c3d4e5f6071"
        );
        assert_eq!(Actor::User(person).user_id(), person);
        assert_eq!(Actor::Elysia.to_string(), "elysia");
        assert_eq!(Actor::Session(12).to_string(), "session:12");
        assert_eq!(
            Actor::Watcher(LinkProvider::Jira).to_string(),
            "watcher:jira"
        );
        assert_eq!(
            Actor::Watcher(LinkProvider::Github).to_string(),
            "watcher:github"
        );
    }
}
