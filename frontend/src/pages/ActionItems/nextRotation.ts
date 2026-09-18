// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Skipping on Next sets an item aside for this visit: it goes to the back of the line
// rather than away, so skipping every item comes back around to the first. Nothing is
// saved; Next's order is the API's to decide.

// The item Next shows given what was skipped: the first not set aside, or the first of all
// once everything has been. Null when Next is empty.
export function pickCurrentItem(items: ActionItem[], skippedIds: string[]) {
  const current = items.find((item) => !skippedIds.includes(item.id)) ?? items[0]
  return current ?? null
}

// The skipped ids after skipping `itemId`. Skipping the last item not yet set aside starts a
// new round, with only that item set aside, so the line starts again from the top.
export function skipItem(items: ActionItem[], skippedIds: string[], itemId: string) {
  const remaining = items.filter((item) => !skippedIds.includes(item.id) && item.id !== itemId)
  if (!remaining.length) {
    return [ itemId ]
  }
  return [ ...skippedIds, itemId ]
}
