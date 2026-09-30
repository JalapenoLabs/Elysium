// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  deletedStudioItemsLoaded,
  selectDeletedStudioItems,
  selectLiveStudioItems,
  selectStudioItemById,
  studioItemDeleted,
  studioItemsLoaded,
  studioItemUpserted,
} from './studioItemsSlice'

// Misc
import { makeStudioItem } from '../testFixtures'

const DELETED_AT = '2026-09-10T00:00:00.000Z'

function ids(items: { id: string }[]) {
  return items.map((item) => item.id)
}

describe('studioItemsSlice', () => {
  it('keeps live and deleted items apart, each load replacing only its own half', () => {
    const store = createAppStore()
    store.dispatch(studioItemsLoaded([ makeStudioItem({ id: 'banana' }) ]))
    store.dispatch(deletedStudioItemsLoaded([ makeStudioItem({ id: 'apple', deletedAt: DELETED_AT }) ]))
    store.dispatch(studioItemsLoaded([ makeStudioItem({ id: 'cherry' }) ]))

    expect(ids(selectLiveStudioItems(store.getState()))).toEqual([ 'cherry' ])
    expect(ids(selectDeletedStudioItems(store.getState()))).toEqual([ 'apple' ])
  })

  it('lists items newest first', () => {
    const store = createAppStore()
    store.dispatch(studioItemsLoaded([
      makeStudioItem({ id: 'old', createdAt: '2026-09-01T00:00:00.000Z' }),
      makeStudioItem({ id: 'new', createdAt: '2026-09-05T00:00:00.000Z' }),
    ]))

    expect(ids(selectLiveStudioItems(store.getState()))).toEqual([ 'new', 'old' ])
  })

  it('moves an upserted item between halves by its deletedAt', () => {
    const store = createAppStore()
    store.dispatch(studioItemsLoaded([ makeStudioItem({ id: 'banana' }) ]))

    store.dispatch(studioItemUpserted(makeStudioItem({ id: 'banana', deletedAt: DELETED_AT })))
    expect(selectLiveStudioItems(store.getState())).toEqual([])
    expect(ids(selectDeletedStudioItems(store.getState()))).toEqual([ 'banana' ])

    // A restore arrives as an upsert with no deletedAt.
    store.dispatch(studioItemUpserted(makeStudioItem({ id: 'banana', title: 'Restored' })))
    expect(ids(selectLiveStudioItems(store.getState()))).toEqual([ 'banana' ])
    expect(selectDeletedStudioItems(store.getState())).toEqual([])
    expect(selectStudioItemById(store.getState(), 'banana')?.title).toBe('Restored')
  })

  it('drops a deleted item from whichever half holds it', () => {
    const store = createAppStore()
    store.dispatch(studioItemsLoaded([ makeStudioItem({ id: 'banana' }) ]))
    store.dispatch(deletedStudioItemsLoaded([ makeStudioItem({ id: 'apple', deletedAt: DELETED_AT }) ]))

    store.dispatch(studioItemDeleted('banana'))
    store.dispatch(studioItemDeleted('apple'))

    expect(selectStudioItemById(store.getState(), 'banana')).toBeUndefined()
    expect(selectStudioItemById(store.getState(), 'apple')).toBeUndefined()
  })
})
