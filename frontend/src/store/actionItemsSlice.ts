// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ActionItem } from '../api/routes/actionItemRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Redux
import { projectDeleted } from './projectsSlice'

// Misc
import { orderNext } from './nextOrder'

// Newest first, matching the API's list.
const actionItemsAdapter = createEntityAdapter<ActionItem>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt),
})

// Live and deleted items are held apart because they load apart: the list answers one or
// the other, and each load replaces only its own half. Deleted items are fetched only
// while a view asks for them.
const initialState = {
  live: actionItemsAdapter.getInitialState(),
  deleted: actionItemsAdapter.getInitialState(),
}

export const actionItemsSlice = createSlice({
  name: 'actionItems',
  initialState,
  reducers: {
    actionItemsLoaded(state, action: PayloadAction<ActionItem[]>) {
      actionItemsAdapter.setAll(state.live, action.payload)
    },
    deletedActionItemsLoaded(state, action: PayloadAction<ActionItem[]>) {
      actionItemsAdapter.setAll(state.deleted, action.payload)
    },
    // Files the item under live or deleted by its `deletedAt`, so a restore moves it back.
    actionItemUpserted(state, action: PayloadAction<ActionItem>) {
      const item = action.payload
      if (item.deletedAt) {
        actionItemsAdapter.removeOne(state.live, item.id)
        actionItemsAdapter.setOne(state.deleted, item)
        return
      }
      actionItemsAdapter.removeOne(state.deleted, item.id)
      actionItemsAdapter.setOne(state.live, item)
    },
    // The event carries only the id, so the item leaves the live half; the deleted half
    // learns of it by reloading, when a view shows it.
    actionItemDeleted(state, action: PayloadAction<string>) {
      actionItemsAdapter.removeOne(state.live, action.payload)
    },
  },
  extraReducers: (builder) => {
    // The API takes a deleted project's items out of it. Live items are announced one by
    // one, but deleted items are not, so both halves drop the id here.
    builder.addCase(projectDeleted, (state, action) => {
      for (const half of [ state.live, state.deleted ]) {
        for (const item of Object.values(half.entities)) {
          if (item.projectIds.includes(action.payload)) {
            item.projectIds = item.projectIds.filter((projectId) => projectId !== action.payload)
          }
        }
      }
    })
  },
})

export const {
  actionItemsLoaded,
  deletedActionItemsLoaded,
  actionItemUpserted,
  actionItemDeleted,
} = actionItemsSlice.actions

export const {
  selectAll: selectLiveActionItems,
} = actionItemsAdapter.getSelectors((state: RootState) => state.actionItems.live)

export const {
  selectAll: selectDeletedActionItems,
} = actionItemsAdapter.getSelectors((state: RootState) => state.actionItems.deleted)

// Live or deleted, whichever holds it.
export function selectActionItemById(state: RootState, itemId: string) {
  return state.actionItems.live.entities[itemId] ?? state.actionItems.deleted.entities[itemId]
}

// Next as of `now`, a millisecond timestamp. Callers pass a `now` that changes only when
// Next should be recomputed, so the result keeps its reference between renders.
export const selectNextActionItems = createSelector(
  [ selectLiveActionItems, (_state: RootState, now: number) => now ],
  (items, now) => orderNext(items, now),
)

// The inbox, oldest first, so triage takes items in the order they arrived.
export const selectInboxActionItems = createSelector(
  [ selectLiveActionItems ],
  (items) => {
    const inbox: ActionItem[] = []
    for (const item of items) {
      if (item.state === 'inbox') {
        inbox.push(item)
      }
    }
    return inbox.reverse()
  },
)

export const selectProjectActionItems = createSelector(
  [ selectLiveActionItems, (_state: RootState, projectId: string) => projectId ],
  (items, projectId) => items.filter((item) => item.projectIds.includes(projectId)),
)

export const selectInitiativeActionItems = createSelector(
  [ selectLiveActionItems, (_state: RootState, initiativeId: string) => initiativeId ],
  (items, initiativeId) => items.filter((item) => item.initiativeIds.includes(initiativeId)),
)

// Titles only, for views that name items, such as history entries. Pair with shallowEqual.
export const selectActionItemTitlesById = createSelector(
  [ selectLiveActionItems, selectDeletedActionItems ],
  (liveItems, deletedItems) => {
    const titlesById: Record<string, string> = {}
    for (const item of deletedItems) {
      titlesById[item.id] = item.title
    }
    for (const item of liveItems) {
      titlesById[item.id] = item.title
    }
    return titlesById
  },
)
