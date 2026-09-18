// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  deletedInitiativesLoaded,
  initiativeDeleted,
  initiativesLoaded,
  initiativeUpserted,
  selectDeletedInitiatives,
  selectInitiativeById,
  selectInitiativeNamesById,
  selectLiveInitiatives,
  selectProjectInitiatives,
} from './initiativesSlice'
import { projectDeleted } from './projectsSlice'

// Misc
import { makeInitiative } from '../testFixtures'

const DELETED_AT = '2026-09-10T00:00:00.000Z'

function ids(initiatives: { id: string }[]) {
  return initiatives.map((initiative) => initiative.id)
}

describe('initiativesSlice', () => {
  it('sorts live initiatives by name', () => {
    const store = createAppStore()
    store.dispatch(initiativesLoaded([
      makeInitiative({ id: 'second', name: 'Ship storage' }),
      makeInitiative({ id: 'first', name: 'Deploy to production' }),
    ]))

    expect(ids(selectLiveInitiatives(store.getState()))).toEqual([ 'first', 'second' ])
  })

  it('moves an initiative between live and deleted by its deletedAt', () => {
    const store = createAppStore()
    store.dispatch(initiativesLoaded([ makeInitiative({ id: 'goal' }) ]))
    store.dispatch(initiativeUpserted(makeInitiative({ id: 'goal', deletedAt: DELETED_AT })))

    expect(selectLiveInitiatives(store.getState())).toEqual([])
    expect(ids(selectDeletedInitiatives(store.getState()))).toEqual([ 'goal' ])

    store.dispatch(initiativeUpserted(makeInitiative({ id: 'goal' })))
    expect(ids(selectLiveInitiatives(store.getState()))).toEqual([ 'goal' ])
    expect(selectDeletedInitiatives(store.getState())).toEqual([])
  })

  it('replaces only the half a load is for', () => {
    const store = createAppStore()
    store.dispatch(deletedInitiativesLoaded([ makeInitiative({ id: 'gone', deletedAt: DELETED_AT }) ]))
    store.dispatch(initiativesLoaded([ makeInitiative({ id: 'live' }) ]))

    expect(selectInitiativeById(store.getState(), 'gone')?.deletedAt).toBe(DELETED_AT)
    expect(selectInitiativeById(store.getState(), 'live')?.deletedAt).toBeNull()
  })

  it('drops a deleted initiative from the live half', () => {
    const store = createAppStore()
    store.dispatch(initiativesLoaded([ makeInitiative({ id: 'goal' }) ]))
    store.dispatch(initiativeDeleted('goal'))

    expect(selectInitiativeById(store.getState(), 'goal')).toBeUndefined()
  })

  it('takes a deleted project out of every initiative', () => {
    const store = createAppStore()
    store.dispatch(initiativesLoaded([ makeInitiative({ id: 'goal', projectIds: [ 'doomed', 'kept' ]}) ]))
    store.dispatch(projectDeleted('doomed'))

    expect(selectInitiativeById(store.getState(), 'goal')?.projectIds).toEqual([ 'kept' ])
  })
})

describe('selectProjectInitiatives', () => {
  it('lists the live initiatives in one project', () => {
    const store = createAppStore()
    store.dispatch(initiativesLoaded([
      makeInitiative({ id: 'in', projectIds: [ 'project' ]}),
      makeInitiative({ id: 'out' }),
    ]))

    expect(ids(selectProjectInitiatives(store.getState(), 'project'))).toEqual([ 'in' ])
  })
})

describe('selectInitiativeNamesById', () => {
  it('names each live initiative and keeps its result while names are unchanged', () => {
    const store = createAppStore()
    store.dispatch(initiativesLoaded([ makeInitiative({ id: 'goal', name: 'Ship it' }) ]))

    const names = selectInitiativeNamesById(store.getState())
    expect(names).toEqual({ goal: 'Ship it' })
    expect(selectInitiativeNamesById(store.getState())).toBe(names)
  })
})
