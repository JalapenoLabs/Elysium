// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { Changeset } from '../../api/routes/changesetRoutes'
import type { Initiative } from '../../api/routes/initiativeRoutes'

// Misc
import { isInNext } from '../../store/nextOrder'
import { DEFAULT_INITIATIVE_STATES } from '../Initiatives/initiativeFilters'
import { DEFAULT_STATES } from './actionItemFilters'

// The Action items area's tabs, in the order they show.
export const ACTION_ITEMS_SECTIONS = [ 'next', 'inbox', 'all', 'initiatives', 'changesets' ] as const
export type ActionItemsSection = typeof ACTION_ITEMS_SECTIONS[number]

// How many rows each of the Action items area's tabs lists when it opens, so its badge and its
// list always agree: Next as of `now`, the inbox, All items under its default filter (what is
// still owed), the initiatives under its default filter, and the changesets waiting for review.
export function countActionItemsSections(
  items: ActionItem[],
  initiatives: Initiative[],
  changesets: Changeset[],
  now: number,
) {
  const counts: Record<ActionItemsSection, number> = {
    next: 0,
    inbox: 0,
    all: 0,
    initiatives: 0,
    changesets: 0,
  }

  for (const item of items) {
    if (isInNext(item, now)) {
      counts.next++
    }
    if (item.state === 'inbox') {
      counts.inbox++
    }
    if (DEFAULT_STATES.includes(item.state)) {
      counts.all++
    }
  }
  for (const initiative of initiatives) {
    if (DEFAULT_INITIATIVE_STATES.includes(initiative.state)) {
      counts.initiatives++
    }
  }
  for (const changeset of changesets) {
    if (changeset.state === 'pending') {
      counts.changesets++
    }
  }

  return counts
}
