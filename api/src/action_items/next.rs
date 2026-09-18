// Copyright © 2026 Jalapeno Labs

//! Next: the order the user works through their open items in.
//!
//! An item is in Next when it is `open`, owned by the user, not deleted, not waiting on
//! anyone, and not snoozed; [`crate::models::action_item::next`] selects those. The order
//! is fixed here, not configured:
//!
//! 1. Overdue items before the rest, as two groups. Every key below orders within a group.
//! 2. Priority: `urgent`, `high`, `normal`, then `low`.
//! 3. Due date, soonest first, so the most overdue leads; items with no due date last.
//! 4. Age, oldest first.
//!
//! The id breaks any remaining tie, so the order never depends on how rows came back.

use std::cmp::Reverse;

use chrono::{DateTime, Utc};

use crate::models::action_item::ActionItem;

/// Sorts `items` into Next's order as of `now`.
pub fn order(items: &mut [ActionItem], now: DateTime<Utc>) {
    items.sort_by_key(|item| {
        let is_overdue = item.due_at.is_some_and(|due_at| due_at < now);
        (
            Reverse(is_overdue),
            item.priority,
            item.due_at.is_none(),
            item.due_at,
            item.created_at,
            item.id,
        )
    });
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use uuid::Uuid;

    use super::*;
    use crate::models::action_item::{ActionItemPriority, ActionItemState, OwnerKind};

    fn item(
        title: &str,
        priority: ActionItemPriority,
        due_at: Option<DateTime<Utc>>,
    ) -> ActionItem {
        let created_at = DateTime::parse_from_rfc3339("2026-09-01T00:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc);
        ActionItem {
            id: Uuid::now_v7(),
            title: title.to_owned(),
            notes: String::new(),
            state: ActionItemState::Open,
            priority,
            due_at,
            snoozed_until: None,
            waiting_on: None,
            owner_kind: OwnerKind::User,
            owner_name: None,
            resolved_at: None,
            dismissed_at: None,
            deleted_at: None,
            created_at,
            updated_at: created_at,
        }
    }

    fn titles(items: &[ActionItem]) -> Vec<&str> {
        items.iter().map(|item| item.title.as_str()).collect()
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-18T12:00:00Z")
            .expect("a valid instant")
            .with_timezone(&Utc)
    }

    #[test]
    fn overdue_items_lead_whatever_their_priority() {
        let now = now();
        let mut items = vec![
            item(
                "urgent, due later",
                ActionItemPriority::Urgent,
                Some(now + TimeDelta::days(1)),
            ),
            item(
                "low, overdue",
                ActionItemPriority::Low,
                Some(now - TimeDelta::hours(1)),
            ),
            item("urgent, undated", ActionItemPriority::Urgent, None),
        ];

        order(&mut items, now);

        assert_eq!(
            titles(&items),
            ["low, overdue", "urgent, due later", "urgent, undated"]
        );
    }

    #[test]
    fn priority_orders_each_group_from_urgent_to_low() {
        let now = now();
        let mut items = vec![
            item("low", ActionItemPriority::Low, None),
            item("normal", ActionItemPriority::Normal, None),
            item("urgent", ActionItemPriority::Urgent, None),
            item("high", ActionItemPriority::High, None),
        ];

        order(&mut items, now);

        assert_eq!(titles(&items), ["urgent", "high", "normal", "low"]);
    }

    #[test]
    fn the_most_overdue_leads_and_undated_items_follow_dated_ones() {
        let now = now();
        let mut items = vec![
            item("undated", ActionItemPriority::High, None),
            item(
                "due tomorrow",
                ActionItemPriority::High,
                Some(now + TimeDelta::days(1)),
            ),
            item(
                "due next week",
                ActionItemPriority::High,
                Some(now + TimeDelta::days(7)),
            ),
            item(
                "a day late",
                ActionItemPriority::High,
                Some(now - TimeDelta::days(1)),
            ),
            item(
                "a week late",
                ActionItemPriority::High,
                Some(now - TimeDelta::days(7)),
            ),
        ];

        order(&mut items, now);

        assert_eq!(
            titles(&items),
            [
                "a week late",
                "a day late",
                "due tomorrow",
                "due next week",
                "undated"
            ]
        );
    }

    #[test]
    fn older_items_lead_among_equals_and_an_item_due_now_is_not_yet_overdue() {
        let now = now();
        let mut newer = item("newer", ActionItemPriority::Normal, Some(now));
        newer.created_at += TimeDelta::hours(1);
        let older = item("older", ActionItemPriority::Normal, Some(now));
        let late = item(
            "late",
            ActionItemPriority::Low,
            Some(now - TimeDelta::seconds(1)),
        );
        let mut items = vec![newer, older, late];

        order(&mut items, now);

        assert_eq!(titles(&items), ["late", "older", "newer"]);
    }
}
