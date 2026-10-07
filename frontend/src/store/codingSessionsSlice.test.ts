// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  codingSessionsLoaded,
  codingSessionsUpserted,
  codingSessionUpserted,
  selectCodingSessionById,
  selectCodingSessions,
  selectCodingSessionsByActionItemId,
  selectCodingSessionsByProjectId,
  selectLatestSessionsByStudioItemId,
  selectSessionCountsByProjectId,
  selectStudioItemSessions,
} from './codingSessionsSlice'
import { satelliteDeleted } from './satellitesSlice'

// Misc
import { makeCodingSession } from '../testFixtures'

function ids(sessions: { id: number }[]) {
  return sessions.map((session) => session.id)
}

describe('codingSessionsLoaded', () => {
  it('replaces the Coding sessions and leaves Studio sessions in place', () => {
    const store = createAppStore()
    store.dispatch(codingSessionsUpserted([
      makeCodingSession({ id: 1, studioItemId: 'banana', projectId: null }),
    ]))
    store.dispatch(codingSessionsLoaded([ makeCodingSession({ id: 2 }) ]))
    store.dispatch(codingSessionsLoaded([ makeCodingSession({ id: 3 }) ]))

    expect(ids(selectCodingSessions(store.getState()))).toEqual([ 3 ])
    expect(ids(selectStudioItemSessions(store.getState(), 'banana'))).toEqual([ 1 ])
  })
})

describe('satelliteDeleted', () => {
  it('keeps the satellite\'s sessions, which the API republishes without it', () => {
    const store = createAppStore()
    store.dispatch(codingSessionsLoaded([ makeCodingSession({ id: 1, satelliteId: 'gone' }) ]))
    store.dispatch(satelliteDeleted('gone'))
    store.dispatch(codingSessionUpserted(makeCodingSession({ id: 1, satelliteId: null })))

    expect(selectCodingSessionById(store.getState(), 1)?.satelliteId).toBeNull()
  })
})

describe('Coding selectors', () => {
  it('never list a Studio session', () => {
    const store = createAppStore()
    store.dispatch(codingSessionsLoaded([ makeCodingSession({ id: 1, projectId: 'shop' }) ]))
    store.dispatch(codingSessionUpserted(makeCodingSession({ id: 2, projectId: 'shop', studioItemId: 'banana' })))

    expect(ids(selectCodingSessions(store.getState()))).toEqual([ 1 ])
    expect(ids(selectCodingSessionsByProjectId(store.getState(), 'shop'))).toEqual([ 1 ])
    expect(selectSessionCountsByProjectId(store.getState())).toEqual({ shop: 1 })
    expect(selectCodingSessionById(store.getState(), 2)).toBeUndefined()
  })

  it('count only sessions that belong to a project', () => {
    const store = createAppStore()
    store.dispatch(codingSessionsLoaded([
      makeCodingSession({ id: 1, projectId: 'shop' }),
      makeCodingSession({ id: 2, projectId: null }),
    ]))

    expect(selectSessionCountsByProjectId(store.getState())).toEqual({ shop: 1 })
  })
})

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

describe('Studio selectors', () => {
  it('list an item\'s sessions oldest first and find its latest', () => {
    const store = createAppStore()
    store.dispatch(codingSessionsUpserted([
      makeCodingSession({ id: 5, studioItemId: 'banana', createdAt: '2026-09-05T00:00:00.000Z' }),
      makeCodingSession({ id: 2, studioItemId: 'banana', createdAt: '2026-09-02T00:00:00.000Z' }),
      makeCodingSession({ id: 3, studioItemId: 'apple', createdAt: '2026-09-03T00:00:00.000Z' }),
      makeCodingSession({ id: 4, createdAt: '2026-09-04T00:00:00.000Z' }),
    ]))

    expect(ids(selectStudioItemSessions(store.getState(), 'banana'))).toEqual([ 2, 5 ])
    const latest = selectLatestSessionsByStudioItemId(store.getState())
    expect(latest.banana?.id).toBe(5)
    expect(latest.apple?.id).toBe(3)
    expect(Object.keys(latest).sort()).toEqual([ 'apple', 'banana' ])
  })
})
