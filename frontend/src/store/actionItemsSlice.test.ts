// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import {
  actionItemDeleted,
  actionItemsLoaded,
  actionItemUpserted,
  deletedActionItemsLoaded,
  selectActionItemById,
  selectActionItemTitlesById,
  selectDeletedActionItems,
  selectInboxActionItems,
  selectLiveActionItems,
  selectNextActionItems,
  selectProjectActionItems,
} from './actionItemsSlice'
import { projectDeleted } from './projectsSlice'

// Misc
import { makeActionItem } from '../testFixtures'

const DELETED_AT = '2026-09-10T00:00:00.000Z'

function ids(items: { id: string }[]) {
  return items.map((item) => item.id)
}

describe('actionItemsSlice', () => {
  it('keeps live and deleted items apart, each load replacing only its own half', () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([ makeActionItem({ id: 'live' }) ]))
    store.dispatch(deletedActionItemsLoaded([ makeActionItem({ id: 'gone', deletedAt: DELETED_AT }) ]))
    store.dispatch(actionItemsLoaded([ makeActionItem({ id: 'fresh' }) ]))

    expect(ids(selectLiveActionItems(store.getState()))).toEqual([ 'fresh' ])
    expect(ids(selectDeletedActionItems(store.getState()))).toEqual([ 'gone' ])
  })

  it('lists live items newest first, as the API does', () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([
      makeActionItem({ id: 'old', createdAt: '2026-09-01T00:00:00.000Z' }),
      makeActionItem({ id: 'new', createdAt: '2026-09-05T00:00:00.000Z' }),
    ]))

    expect(ids(selectLiveActionItems(store.getState()))).toEqual([ 'new', 'old' ])
  })

  it('moves an upserted item between halves by its deletedAt', () => {
    const store = createAppStore()
    store.dispatch(deletedActionItemsLoaded([ makeActionItem({ id: 'item', deletedAt: DELETED_AT }) ]))

    // A restore arrives as an upsert with no deletedAt.
    store.dispatch(actionItemUpserted(makeActionItem({ id: 'item', title: 'Restored' })))
    expect(ids(selectLiveActionItems(store.getState()))).toEqual([ 'item' ])
    expect(selectDeletedActionItems(store.getState())).toEqual([])
    expect(selectActionItemById(store.getState(), 'item')?.title).toBe('Restored')

    store.dispatch(actionItemUpserted(makeActionItem({ id: 'item', deletedAt: DELETED_AT })))
    expect(selectLiveActionItems(store.getState())).toEqual([])
    expect(ids(selectDeletedActionItems(store.getState()))).toEqual([ 'item' ])
  })

  it('drops a deleted item from the live half', () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([ makeActionItem({ id: 'item' }) ]))
    store.dispatch(actionItemDeleted('item'))

    expect(selectLiveActionItems(store.getState())).toEqual([])
    expect(selectActionItemById(store.getState(), 'item')).toBeUndefined()
  })

  it('takes a deleted project out of every item, live or deleted', () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([ makeActionItem({ id: 'live', projectIds: [ 'doomed', 'kept' ]}) ]))
    store.dispatch(deletedActionItemsLoaded([
      makeActionItem({ id: 'gone', projectIds: [ 'doomed' ], deletedAt: DELETED_AT }),
    ]))
    store.dispatch(projectDeleted('doomed'))

    expect(selectActionItemById(store.getState(), 'live')?.projectIds).toEqual([ 'kept' ])
    expect(selectActionItemById(store.getState(), 'gone')?.projectIds).toEqual([])
  })
})

describe('selectNextActionItems', () => {
  it('orders the live items in Next and keeps its result while nothing changes', () => {
    const store = createAppStore()
    const now = Date.parse('2026-09-18T12:00:00Z')
    store.dispatch(actionItemsLoaded([
      makeActionItem({ id: 'normal' }),
      makeActionItem({ id: 'urgent', priority: 'urgent' }),
      makeActionItem({ id: 'inbox', state: 'inbox' }),
    ]))

    const first = selectNextActionItems(store.getState(), now)
    expect(ids(first)).toEqual([ 'urgent', 'normal' ])
    expect(selectNextActionItems(store.getState(), now)).toBe(first)
  })
})

describe('selectInboxActionItems', () => {
  it('lists inbox items oldest first', () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([
      makeActionItem({ id: 'newer', state: 'inbox', createdAt: '2026-09-03T00:00:00.000Z' }),
      makeActionItem({ id: 'open' }),
      makeActionItem({ id: 'older', state: 'inbox', createdAt: '2026-09-02T00:00:00.000Z' }),
    ]))

    expect(ids(selectInboxActionItems(store.getState()))).toEqual([ 'older', 'newer' ])
  })
})

describe('selectProjectActionItems', () => {
  it('lists the live items in one project', () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([
      makeActionItem({ id: 'in', projectIds: [ 'project' ]}),
      makeActionItem({ id: 'out', projectIds: [ 'other' ]}),
    ]))

    expect(ids(selectProjectActionItems(store.getState(), 'project'))).toEqual([ 'in' ])
  })
})

describe('selectActionItemTitlesById', () => {
  it('names live and deleted items', () => {
    const store = createAppStore()
    store.dispatch(actionItemsLoaded([ makeActionItem({ id: 'live', title: 'Reply to Sam' }) ]))
    store.dispatch(deletedActionItemsLoaded([
      makeActionItem({ id: 'gone', title: 'Old task', deletedAt: DELETED_AT }),
    ]))

    expect(selectActionItemTitlesById(store.getState())).toEqual({
      live: 'Reply to Sam',
      gone: 'Old task',
    })
  })
})
