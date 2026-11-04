// Copyright © 2026 Jalapeno Labs

import type { InitiativeFilters } from './initiativeFilters'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeInitiative } from '../../testFixtures'
import { NO_PROJECT } from '../ActionItems/actionItemFilters'
import {
  DEFAULT_INITIATIVE_STATES,
  filterInitiatives,
  fromInitiativeFilterValues,
  readInitiativeFilters,
  toInitiativeFilterValues,
  writeInitiativeFilters,
} from './initiativeFilters'

const ANYTHING: InitiativeFilters = {
  states: [],
  project: null,
  deleted: false,
}

describe('readInitiativeFilters', () => {
  it('shows the active initiatives when the address names no filter', () => {
    expect(readInitiativeFilters(new URLSearchParams())).toEqual({
      ...ANYTHING,
      states: DEFAULT_INITIATIVE_STATES,
    })
  })

  it('reads every filter and ignores states it does not know', () => {
    const params = new URLSearchParams('state=achieved,paused&project=p1&deleted=true')

    expect(readInitiativeFilters(params)).toEqual({ states: [ 'achieved' ], project: 'p1', deleted: true })
  })

  it('reads state=all as every state', () => {
    expect(readInitiativeFilters(new URLSearchParams('state=all')).states).toEqual([])
  })
})

describe('writeInitiativeFilters', () => {
  it('leaves the defaults out of the address', () => {
    expect(writeInitiativeFilters({ ...ANYTHING, states: [ 'active' ]}).toString()).toBe('')
  })

  it('round-trips through the address', () => {
    const filters: InitiativeFilters = { states: [], project: NO_PROJECT, deleted: true }

    expect(readInitiativeFilters(writeInitiativeFilters(filters))).toEqual(filters)
  })
})

describe('initiative filter values', () => {
  it('round-trip through the drawer', () => {
    const filters: InitiativeFilters = { states: [ 'achieved', 'abandoned' ], project: 'p1', deleted: true }

    expect(fromInitiativeFilterValues(toInitiativeFilterValues(filters))).toEqual(filters)
  })

  it('show every state and the live list as no filter at all', () => {
    expect(toInitiativeFilterValues(ANYTHING)).toEqual({ state: null, project: null, deleted: null })
  })

  it('take a removed state tag as every state', () => {
    const values = toInitiativeFilterValues({ ...ANYTHING, states: DEFAULT_INITIATIVE_STATES })
    const filters = fromInitiativeFilterValues({ ...values, state: null })

    expect(writeInitiativeFilters(filters).toString()).toBe('state=all')
  })
})

describe('filterInitiatives', () => {
  const initiatives = [
    makeInitiative({ id: 'active', projectIds: [ 'p1' ]}),
    makeInitiative({ id: 'achieved', state: 'achieved' }),
    makeInitiative({ id: 'elsewhere', projectIds: [ 'p2' ]}),
  ]

  function ids(filters: InitiativeFilters) {
    return filterInitiatives(initiatives, filters).map((initiative) => initiative.id)
  }

  it('keeps everything when nothing is filtered', () => {
    expect(ids(ANYTHING)).toEqual([ 'active', 'achieved', 'elsewhere' ])
  })

  it('filters by state', () => {
    expect(ids({ ...ANYTHING, states: [ 'achieved' ]})).toEqual([ 'achieved' ])
  })

  it('filters by one project, or by none', () => {
    expect(ids({ ...ANYTHING, project: 'p1' })).toEqual([ 'active' ])
    expect(ids({ ...ANYTHING, project: NO_PROJECT })).toEqual([ 'achieved' ])
  })
})
