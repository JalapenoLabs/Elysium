// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  historyAppended,
  historyLoaded,
  selectActionItemHistory,
  selectInitiativeHistory,
} from './actionItemHistorySlice'

// Misc
import { makeHistoryEntry } from '../testFixtures'

function ids(entries: { id: string }[]) {
  return entries.map((entry) => entry.id)
}

describe('actionItemHistorySlice', () => {
  it('keeps an entry that arrived live while the history loaded, without duplicating it', () => {
    const store = createAppStore()
    const live = makeHistoryEntry({ id: 'live', actionItemId: 'item', createdAt: '2026-09-03T00:00:00.000Z' })
    store.dispatch(historyAppended(live))
    store.dispatch(historyLoaded([
      makeHistoryEntry({ id: 'created', actionItemId: 'item', createdAt: '2026-09-01T00:00:00.000Z' }),
      live,
    ]))

    expect(ids(selectActionItemHistory(store.getState(), 'item'))).toEqual([ 'created', 'live' ])
  })

  it('shows an entry naming both an item and an initiative in both histories', () => {
    const store = createAppStore()
    store.dispatch(historyAppended(makeHistoryEntry({
      id: 'joined',
      kind: 'initiative_joined',
      actionItemId: 'item',
      initiativeId: 'goal',
    })))
    store.dispatch(historyAppended(makeHistoryEntry({ id: 'renamed', kind: 'updated', initiativeId: 'goal' })))

    expect(ids(selectActionItemHistory(store.getState(), 'item'))).toEqual([ 'joined' ])
    expect(ids(selectInitiativeHistory(store.getState(), 'goal'))).toEqual([ 'joined', 'renamed' ])
  })
})
