// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeActionItem, makeChangeset, makeInitiative } from '../../testFixtures'
import { countActionItemsSections } from './actionItemsSectionCounts'

const NOW = Date.parse('2026-11-04T12:00:00Z')

describe('countActionItemsSections', () => {
  it('counts zero for every tab when nothing is there', () => {
    expect(countActionItemsSections([], [], [], NOW)).toEqual({
      next: 0,
      inbox: 0,
      all: 0,
      initiatives: 0,
      changesets: 0,
    })
  })

  it('counts what each tab lists when it opens', () => {
    const items = [
      makeActionItem({ id: 'open' }),
      makeActionItem({ id: 'triage', state: 'inbox' }),
      // Owed, so All items lists them, but not in Next.
      makeActionItem({ id: 'waiting', waitingOn: 'Sam' }),
      makeActionItem({ id: 'snoozed', snoozedUntil: '2026-11-05T09:00:00Z' }),
      // Done with, so only All items' other filters list them.
      makeActionItem({ id: 'resolved', state: 'resolved' }),
      makeActionItem({ id: 'dismissed', state: 'dismissed' }),
    ]
    const initiatives = [
      makeInitiative({ id: 'launch' }),
      makeInitiative({ id: 'migration', state: 'achieved' }),
    ]
    const changesets = [
      makeChangeset({ id: 'follow-ups' }),
      makeChangeset({ id: 'applied', state: 'applied' }),
    ]

    expect(countActionItemsSections(items, initiatives, changesets, NOW)).toEqual({
      next: 1,
      inbox: 1,
      all: 4,
      initiatives: 1,
      changesets: 1,
    })
  })

  it('brings a snoozed item back into Next once its snooze runs out', () => {
    const items = [ makeActionItem({ id: 'snoozed', snoozedUntil: '2026-11-04T11:00:00Z' }) ]
    expect(countActionItemsSections(items, [], [], NOW).next).toBe(1)
  })
})
