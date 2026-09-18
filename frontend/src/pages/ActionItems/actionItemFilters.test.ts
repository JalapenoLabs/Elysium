// Copyright © 2026 Jalapeno Labs

import type { ActionItemFilters } from './actionItemFilters'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeActionItem } from '../../testFixtures'
import {
  DEFAULT_STATES,
  filterActionItems,
  NO_PROJECT,
  readActionItemFilters,
  writeActionItemFilters,
} from './actionItemFilters'

const NOW = Date.parse('2026-09-18T12:00:00.000Z')

const ANYTHING: ActionItemFilters = {
  states: [],
  project: null,
  initiative: null,
  waiting: 'any',
  snoozed: 'any',
  deleted: false,
}

function ids(items: { id: string }[]) {
  return items.map((item) => item.id)
}

describe('readActionItemFilters', () => {
  it('shows the inbox and open items when the address names no filter', () => {
    expect(readActionItemFilters(new URLSearchParams())).toEqual({
      ...ANYTHING,
      states: DEFAULT_STATES,
    })
  })

  it('reads every filter and ignores states it does not know', () => {
    const params = new URLSearchParams(
      'state=resolved,archived&project=p1&initiative=i1&waiting=yes&snoozed=no&deleted=true',
    )

    expect(readActionItemFilters(params)).toEqual({
      states: [ 'resolved' ],
      project: 'p1',
      initiative: 'i1',
      waiting: 'yes',
      snoozed: 'no',
      deleted: true,
    })
  })

  it('reads state=all as every state', () => {
    expect(readActionItemFilters(new URLSearchParams('state=all')).states).toEqual([])
  })
})

describe('writeActionItemFilters', () => {
  it('leaves the defaults out of the address', () => {
    const params = writeActionItemFilters({ ...ANYTHING, states: [ 'open', 'inbox' ]})

    expect(params.toString()).toBe('')
  })

  it('round-trips through the address', () => {
    const filters: ActionItemFilters = {
      states: [],
      project: NO_PROJECT,
      initiative: 'i1',
      waiting: 'no',
      snoozed: 'yes',
      deleted: true,
    }

    expect(readActionItemFilters(writeActionItemFilters(filters))).toEqual(filters)
  })
})

describe('filterActionItems', () => {
  const items = [
    makeActionItem({ id: 'open', projectIds: [ 'p1' ], initiativeIds: [ 'i1' ]}),
    makeActionItem({ id: 'resolved', state: 'resolved' }),
    makeActionItem({ id: 'waiting', waitingOn: 'Sam', projectIds: [ 'p2' ]}),
    makeActionItem({ id: 'snoozed', snoozedUntil: '2026-09-19T00:00:00.000Z' }),
    makeActionItem({ id: 'snooze over', snoozedUntil: '2026-09-17T00:00:00.000Z' }),
  ]

  it('keeps everything when nothing is filtered', () => {
    expect(ids(filterActionItems(items, ANYTHING, NOW))).toEqual(ids(items))
  })

  it('filters by state', () => {
    expect(ids(filterActionItems(items, { ...ANYTHING, states: [ 'resolved' ]}, NOW))).toEqual([ 'resolved' ])
  })

  it('filters by one project, or by none', () => {
    expect(ids(filterActionItems(items, { ...ANYTHING, project: 'p1' }, NOW))).toEqual([ 'open' ])
    expect(ids(filterActionItems(items, { ...ANYTHING, project: NO_PROJECT }, NOW))).toEqual([
      'resolved',
      'snoozed',
      'snooze over',
    ])
  })

  it('filters by initiative', () => {
    expect(ids(filterActionItems(items, { ...ANYTHING, initiative: 'i1' }, NOW))).toEqual([ 'open' ])
  })

  it('filters by waiting either way', () => {
    expect(ids(filterActionItems(items, { ...ANYTHING, waiting: 'yes' }, NOW))).toEqual([ 'waiting' ])
    expect(ids(filterActionItems(items, { ...ANYTHING, waiting: 'no' }, NOW))).not.toContain('waiting')
  })

  it('counts a snooze as held only until it runs out', () => {
    expect(ids(filterActionItems(items, { ...ANYTHING, snoozed: 'yes' }, NOW))).toEqual([ 'snoozed' ])
    expect(ids(filterActionItems(items, { ...ANYTHING, snoozed: 'no' }, NOW))).toContain('snooze over')
  })
})
