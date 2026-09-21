// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  changesetsLoaded,
  changesetUpserted,
  selectChangesetById,
  selectChangesets,
  selectPendingChangesets,
} from './changesetsSlice'

// Misc
import { makeChangeset } from '../testFixtures'

// UUIDv7-like ids, which sort the way changesets were proposed.
const OLDER = '01920000-0000-7000-8000-000000000001'
const NEWER = '01920000-0000-7000-8000-000000000002'

describe('changesetsSlice', () => {
  it('lists changesets newest first and counts only the pending ones', () => {
    const store = createAppStore()
    store.dispatch(changesetsLoaded([
      makeChangeset({ id: OLDER, state: 'applied' }),
      makeChangeset({ id: NEWER }),
    ]))

    const state = store.getState()
    expect(selectChangesets(state).map((changeset) => changeset.id)).toEqual([ NEWER, OLDER ])
    expect(selectPendingChangesets(state).map((changeset) => changeset.id)).toEqual([ NEWER ])
  })

  it('replaces a changeset whole when it is decided, applied, or undone', () => {
    const store = createAppStore()
    store.dispatch(changesetsLoaded([ makeChangeset({ id: NEWER }) ]))
    store.dispatch(changesetUpserted(makeChangeset({ id: NEWER, state: 'applied' })))

    expect(selectChangesetById(store.getState(), NEWER)?.state).toBe('applied')
    expect(selectPendingChangesets(store.getState())).toEqual([])
  })

  it('keeps the pending list reference while nothing changes', () => {
    const store = createAppStore()
    store.dispatch(changesetsLoaded([ makeChangeset({ id: NEWER }) ]))
    const first = selectPendingChangesets(store.getState())
    expect(selectPendingChangesets(store.getState())).toBe(first)
  })
})
