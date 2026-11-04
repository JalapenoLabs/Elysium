// Copyright © 2026 Jalapeno Labs

import type { SessionSummary } from './sessionPresentation'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeCodingSession } from '../../testFixtures'
import {
  filterSessionSummaries,
  fromSessionFilterValues,
  readSessionFilters,
  toSessionFilterValues,
  writeSessionFilters,
} from './sessionFilters'

function makeSummary(id: number, state: SessionSummary['state'], projectId: string): SessionSummary {
  return {
    session: makeCodingSession({ id, projectId }),
    projectName: projectId,
    state,
    stateLabel: state,
    lastActivity: '',
  }
}

describe('readSessionFilters', () => {
  it('filters nothing when the address names no filter', () => {
    expect(readSessionFilters(new URLSearchParams())).toEqual({ states: [], project: null })
  })

  it('reads both filters and ignores states it does not know', () => {
    const params = new URLSearchParams('state=running,paused&project=p1')

    expect(readSessionFilters(params)).toEqual({ states: [ 'running' ], project: 'p1' })
  })
})

describe('writeSessionFilters', () => {
  it('keeps what else the address holds, and leaves out filters that are off', () => {
    const params = new URLSearchParams('item=i1&state=idle')

    expect(writeSessionFilters(params, { states: [], project: 'p1' }).toString()).toBe('item=i1&project=p1')
  })

  it('round-trips through the address and the drawer', () => {
    const filters = { states: [ 'queued' as const, 'running' as const ], project: 'p1' }
    const params = writeSessionFilters(new URLSearchParams(), filters)

    expect(readSessionFilters(params)).toEqual(filters)
    expect(fromSessionFilterValues(toSessionFilterValues(filters))).toEqual(filters)
  })

  it('takes a removed tag as that filter off', () => {
    const values = toSessionFilterValues({ states: [ 'idle' ], project: 'p1' })

    expect(fromSessionFilterValues({ ...values, project: null })).toEqual({ states: [ 'idle' ], project: null })
  })
})

describe('filterSessionSummaries', () => {
  const summaries = [
    makeSummary(1, 'idle', 'p1'),
    makeSummary(2, 'running', 'p1'),
    makeSummary(3, 'running', 'p2'),
  ]

  function ids(filters: Parameters<typeof filterSessionSummaries>[1]) {
    return filterSessionSummaries(summaries, filters).map((summary) => summary.session.id)
  }

  it('keeps every session when nothing is filtered', () => {
    expect(ids({ states: [], project: null })).toEqual([ 1, 2, 3 ])
  })

  it('filters by state and project together', () => {
    expect(ids({ states: [ 'running' ], project: null })).toEqual([ 2, 3 ])
    expect(ids({ states: [ 'running' ], project: 'p1' })).toEqual([ 2 ])
  })
})
