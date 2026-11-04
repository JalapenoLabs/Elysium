// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import {
  countKnowledgeFilters,
  NO_KNOWLEDGE_FILTERS,
  readKnowledgeFilters,
  readKnowledgeQuery,
  toKnowledgeQuery,
  writeKnowledgeSearch,
} from './knowledgeFilters'

describe('readKnowledgeFilters', () => {
  it('reads every filter the address holds', () => {
    const params = new URLSearchParams(
      'q=budget&source=slack,outlook&project=p1&person=u1&from=2026-11-01&to=2026-11-30',
    )

    expect(readKnowledgeQuery(params)).toBe('budget')
    expect(readKnowledgeFilters(params)).toEqual({
      sources: [ 'slack', 'outlook' ],
      projectId: 'p1',
      personId: 'u1',
      fromDay: '2026-11-01',
      toDay: '2026-11-30',
    })
  })

  it('narrows nothing when the address holds no filter', () => {
    expect(readKnowledgeFilters(new URLSearchParams())).toEqual(NO_KNOWLEDGE_FILTERS)
    expect(readKnowledgeQuery(new URLSearchParams())).toBe('')
  })

  it('ignores a day it cannot read rather than sending it on', () => {
    const filters = readKnowledgeFilters(new URLSearchParams('from=yesterday&to=2026-02-30'))

    expect(filters.fromDay).toBeNull()
    expect(filters.toDay).toBeNull()
  })
})

describe('writeKnowledgeSearch', () => {
  it('writes only the filters that narrow, keeping the other parameters', () => {
    const next = writeKnowledgeSearch(
      new URLSearchParams('tab=x&person=old'),
      'launch',
      { ...NO_KNOWLEDGE_FILTERS, sources: [ 'meeting', 'learning' ], fromDay: '2026-10-01' },
    )

    expect(next.toString()).toBe('tab=x&q=launch&source=meeting%2Clearning&from=2026-10-01')
  })

  it('reads back what it wrote', () => {
    const filters = {
      sources: [ 'teams' ],
      projectId: 'p2',
      personId: 'u2',
      fromDay: '2026-01-05',
      toDay: '2026-01-06',
    }
    const written = writeKnowledgeSearch(new URLSearchParams(), 'WAY-123', filters)

    expect(readKnowledgeQuery(written)).toBe('WAY-123')
    expect(readKnowledgeFilters(written)).toEqual(filters)
  })
})

describe('countKnowledgeFilters', () => {
  it('counts each source and each other filter on', () => {
    expect(countKnowledgeFilters(NO_KNOWLEDGE_FILTERS)).toBe(0)
    expect(countKnowledgeFilters({
      ...NO_KNOWLEDGE_FILTERS,
      sources: [ 'slack', 'teams' ],
      personId: 'u1',
      toDay: '2026-11-01',
    })).toBe(4)
  })
})

describe('toKnowledgeQuery', () => {
  it('asks from the start of the first day to the end of the last, in the viewer\'s zone', () => {
    // Daylight saving ends in New York on November 1st 2026, so the two days start at
    // different offsets.
    const query = toKnowledgeQuery(
      'budget',
      { ...NO_KNOWLEDGE_FILTERS, fromDay: '2026-11-01', toDay: '2026-11-01' },
      'America/New_York',
      10,
    )

    expect(query).toEqual({
      q: 'budget',
      sources: [],
      projectId: null,
      personId: null,
      from: '2026-11-01T04:00:00.000Z',
      to: '2026-11-02T05:00:00.000Z',
      limit: 10,
    })
  })

  it('leaves an open end open', () => {
    const query = toKnowledgeQuery('x', NO_KNOWLEDGE_FILTERS, 'UTC')

    expect(query.from).toBeNull()
    expect(query.to).toBeNull()
  })
})
