// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  codingSessionsLoaded,
  codingSessionUpserted,
  selectCodingSessionsByActionItemId,
} from './codingSessionsSlice'

// Misc
import { makeCodingSession } from '../testFixtures'

function ids(sessions: { id: number }[]) {
  return sessions.map((session) => session.id)
}

describe('selectCodingSessionsByActionItemId', () => {
  it('lists the sessions started from one item, newest first', () => {
    const store = createAppStore()
    store.dispatch(codingSessionsLoaded([
      makeCodingSession({ id: 1, actionItemId: 'login', createdAt: '2026-09-01T00:00:00.000Z' }),
      makeCodingSession({ id: 2, actionItemId: 'docs', createdAt: '2026-09-02T00:00:00.000Z' }),
      makeCodingSession({ id: 3, actionItemId: null, createdAt: '2026-09-03T00:00:00.000Z' }),
      makeCodingSession({ id: 4, actionItemId: 'login', createdAt: '2026-09-04T00:00:00.000Z' }),
    ]))

    expect(ids(selectCodingSessionsByActionItemId(store.getState(), 'login'))).toEqual([ 4, 1 ])
    expect(selectCodingSessionsByActionItemId(store.getState(), 'nothing')).toEqual([])
  })

  it('picks up a session started from the item as it arrives', () => {
    const store = createAppStore()
    store.dispatch(codingSessionsLoaded([]))
    store.dispatch(codingSessionUpserted(makeCodingSession({ id: 7, actionItemId: 'login' })))

    expect(ids(selectCodingSessionsByActionItemId(store.getState(), 'login'))).toEqual([ 7 ])
  })
})
