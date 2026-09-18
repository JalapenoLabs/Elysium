// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemPriority } from '../api/routes/actionItemRoutes'

// Next, computed where the items already are. This mirrors `next` in
// api/src/models/action_item.rs (which items) and api/src/action_items/next.rs (their
// order), the way environmentPresentation.ts mirrors the API's key rules. Computing it here
// lets acting on an item move to the next one at once, and lets a snooze that runs out bring
// its item back, which no server event announces. Keep the two in step.

const priorityRank = {
  urgent: 0,
  high: 1,
  normal: 2,
  low: 3,
} as const satisfies Record<ActionItemPriority, number>

// An item is in Next when it is open, the user's, not deleted, not waiting on anyone, and
// not snoozed past `now`.
export function isInNext(item: ActionItem, now: number) {
  if (item.state !== 'open' || item.owner.kind !== 'user' || item.deletedAt || item.waitingOn) {
    return false
  }
  return !item.snoozedUntil || Date.parse(item.snoozedUntil) <= now
}

// Overdue items first as their own group, then by priority, due date (soonest first, undated
// last), and age (oldest first). The id breaks any remaining tie. An item due exactly now
// is not yet overdue.
export function compareNext(first: ActionItem, second: ActionItem, now: number) {
  const firstDue = first.dueAt
    ? Date.parse(first.dueAt)
    : null
  const secondDue = second.dueAt
    ? Date.parse(second.dueAt)
    : null

  const firstOverdue = firstDue !== null && firstDue < now
  const secondOverdue = secondDue !== null && secondDue < now
  if (firstOverdue !== secondOverdue) {
    return firstOverdue
      ? -1
      : 1
  }

  const byPriority = priorityRank[first.priority] - priorityRank[second.priority]
  if (byPriority) {
    return byPriority
  }

  if (firstDue !== secondDue) {
    if (firstDue === null) {
      return 1
    }
    if (secondDue === null) {
      return -1
    }
    return firstDue - secondDue
  }

  const byAge = Date.parse(first.createdAt) - Date.parse(second.createdAt)
  if (byAge) {
    return byAge
  }

  // Code point order, which for lowercase UUIDs is the API's byte order.
  if (first.id === second.id) {
    return 0
  }
  if (first.id < second.id) {
    return -1
  }
  return 1
}

// The items in Next, in the order the user works through them.
export function orderNext(items: ActionItem[], now: number) {
  const next: ActionItem[] = []
  for (const item of items) {
    if (isInNext(item, now)) {
      next.push(item)
    }
  }
  return next.sort((first, second) => compareNext(first, second, now))
}
