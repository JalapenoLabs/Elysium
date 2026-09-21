// Copyright © 2026 Jalapeno Labs

//! What a provider's change does to a linked item. See `docs/action-items.md`, "An action
//! item is a commitment, not a copy".
//!
//! The watcher acts on a change of a link's state, never on the state alone: it compares
//! what the provider reports now with what the link last recorded. So an item the user
//! reopened stays open while its issue stays done, and a change the watcher reads twice, or
//! one Elysium made itself and already recorded, does nothing.

use crate::models::action_item::{ActionItemState, Owner};
use crate::models::action_item_link::LinkState;

/// What the item does because its link's state changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reaction {
    Nothing,
    /// The link closed, merged, or reached done: the item resolves.
    Resolve,
    /// A GitHub issue closed as not planned: the item is dismissed.
    Dismiss,
    /// A linked issue reopened: a resolved item returns to open.
    Reopen,
    /// A pull request closed without merging: recorded, and the item stays as it is, since
    /// the work it stands for may still be owed.
    RecordClosedUnmerged,
}

/// The item's reaction to its link moving from `previous` to `current`, given the item's
/// own state.
pub fn react(previous: LinkState, current: LinkState, item: ActionItemState) -> Reaction {
    if previous == current {
        return Reaction::Nothing;
    }
    let is_pending = matches!(item, ActionItemState::Inbox | ActionItemState::Open);
    match current {
        LinkState::Done | LinkState::Merged if is_pending => Reaction::Resolve,
        LinkState::NotPlanned if is_pending => Reaction::Dismiss,
        LinkState::ClosedUnmerged => Reaction::RecordClosedUnmerged,
        // Only an issue coming back from done or not planned reopens its item, and only a
        // resolved one: a dismissed item stays dismissed, and a pull request reopened after
        // closing unmerged never resolved anything.
        LinkState::Open
            if matches!(previous, LinkState::Done | LinkState::NotPlanned)
                && item == ActionItemState::Resolved =>
        {
            Reaction::Reopen
        }
        _otherwise => Reaction::Nothing,
    }
}

/// The owner a primary link's item takes because the thing's assignee changed from
/// `previous` to `current`, or `None` when it keeps the one it has.
///
/// Only a change moves the owner, so an owner the user set by hand stays until the
/// assignee changes again.
pub fn owner_change(
    is_primary: bool,
    previous: &Owner,
    current: &Owner,
    item: &Owner,
) -> Option<Owner> {
    if !is_primary || previous == current || item == current {
        return None;
    }
    Some(current.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ITEM_STATES: [ActionItemState; 4] = [
        ActionItemState::Inbox,
        ActionItemState::Open,
        ActionItemState::Resolved,
        ActionItemState::Dismissed,
    ];

    #[test]
    fn a_link_that_reaches_done_or_merges_resolves_an_item_still_owed() {
        for current in [LinkState::Done, LinkState::Merged] {
            assert_eq!(
                react(LinkState::Open, current, ActionItemState::Open),
                Reaction::Resolve
            );
            assert_eq!(
                react(LinkState::Open, current, ActionItemState::Inbox),
                Reaction::Resolve
            );
            assert_eq!(
                react(LinkState::Open, current, ActionItemState::Resolved),
                Reaction::Nothing,
                "already resolved, as when Elysium closed the issue itself"
            );
            assert_eq!(
                react(LinkState::Open, current, ActionItemState::Dismissed),
                Reaction::Nothing,
                "a dismissed item is not resolved behind the user's back"
            );
        }
    }

    #[test]
    fn an_issue_closed_as_not_planned_dismisses() {
        assert_eq!(
            react(
                LinkState::Open,
                LinkState::NotPlanned,
                ActionItemState::Open
            ),
            Reaction::Dismiss
        );
        assert_eq!(
            react(
                LinkState::Open,
                LinkState::NotPlanned,
                ActionItemState::Resolved
            ),
            Reaction::Nothing
        );
    }

    #[test]
    fn a_reopened_issue_returns_only_a_resolved_item_to_open() {
        assert_eq!(
            react(LinkState::Done, LinkState::Open, ActionItemState::Resolved),
            Reaction::Reopen
        );
        assert_eq!(
            react(
                LinkState::NotPlanned,
                LinkState::Open,
                ActionItemState::Resolved
            ),
            Reaction::Reopen
        );
        assert_eq!(
            react(LinkState::Done, LinkState::Open, ActionItemState::Dismissed),
            Reaction::Nothing,
            "a dismissed item stays dismissed"
        );
        assert_eq!(
            react(LinkState::Done, LinkState::Open, ActionItemState::Open),
            Reaction::Nothing
        );
    }

    #[test]
    fn a_pull_request_closed_unmerged_resolves_nothing_and_is_recorded() {
        for item in ITEM_STATES {
            assert_eq!(
                react(LinkState::Open, LinkState::ClosedUnmerged, item),
                Reaction::RecordClosedUnmerged,
                "{item:?}"
            );
        }
        assert_eq!(
            react(
                LinkState::ClosedUnmerged,
                LinkState::Open,
                ActionItemState::Resolved
            ),
            Reaction::Nothing,
            "a pull request reopened after closing unmerged never resolved the item"
        );
    }

    #[test]
    fn a_state_read_twice_does_nothing() {
        for state in [
            LinkState::Open,
            LinkState::Done,
            LinkState::NotPlanned,
            LinkState::Merged,
            LinkState::ClosedUnmerged,
        ] {
            for item in ITEM_STATES {
                assert_eq!(react(state, state, item), Reaction::Nothing, "{state:?}");
            }
        }
    }

    #[test]
    fn the_owner_follows_a_primary_links_assignee_only_when_it_changes() {
        let sam = Owner::Other {
            name: "Sam".to_owned(),
        };

        assert_eq!(
            owner_change(true, &Owner::Nobody, &Owner::User, &Owner::Nobody),
            Some(Owner::User),
            "assigned to the credential's own account: the item is the user's"
        );
        assert_eq!(
            owner_change(true, &Owner::User, &sam, &Owner::User),
            Some(sam.clone())
        );
        assert_eq!(
            owner_change(false, &Owner::User, &sam, &Owner::User),
            None,
            "a link that is not primary never moves the owner"
        );
        assert_eq!(
            owner_change(true, &sam, &sam, &Owner::User),
            None,
            "an owner the user set by hand stays while the assignee stays"
        );
        assert_eq!(owner_change(true, &Owner::User, &sam, &sam), None);
    }
}
