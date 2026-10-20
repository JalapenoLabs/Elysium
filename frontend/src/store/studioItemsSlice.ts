// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { StudioItem } from '../api/routes/studioRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Newest first, matching the API's list.
const studioItemsAdapter = createEntityAdapter<StudioItem>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt),
})

// Live and deleted items are held apart because they load apart: the list answers one or
// the other, and each load replaces only its own half. Deleted items are fetched only
// while a view asks for them.
const initialState = {
  live: studioItemsAdapter.getInitialState(),
  deleted: studioItemsAdapter.getInitialState(),
}

export const studioItemsSlice = createSlice({
  name: 'studioItems',
  initialState,
  reducers: {
    studioItemsLoaded(state, action: PayloadAction<StudioItem[]>) {
      studioItemsAdapter.setAll(state.live, action.payload)
    },
    deletedStudioItemsLoaded(state, action: PayloadAction<StudioItem[]>) {
      studioItemsAdapter.setAll(state.deleted, action.payload)
    },
    // Files the item under live or deleted by its `deletedAt`, so a restore moves it back.
    studioItemUpserted(state, action: PayloadAction<StudioItem>) {
      const item = action.payload
      if (item.deletedAt) {
        studioItemsAdapter.removeOne(state.live, item.id)
        studioItemsAdapter.setOne(state.deleted, item)
        return
      }
      studioItemsAdapter.removeOne(state.deleted, item.id)
      studioItemsAdapter.setOne(state.live, item)
    },
    // The event carries only the id and is sent for soft and permanent deletes alike, so
    // the item leaves whichever half holds it. A softly deleted one returns to the deleted
    // half when that list reloads.
    studioItemDeleted(state, action: PayloadAction<string>) {
      studioItemsAdapter.removeOne(state.live, action.payload)
      studioItemsAdapter.removeOne(state.deleted, action.payload)
    },
  },
})

export const {
  studioItemsLoaded,
  deletedStudioItemsLoaded,
  studioItemUpserted,
  studioItemDeleted,
} = studioItemsSlice.actions

export const {
  selectAll: selectLiveStudioItems,
} = studioItemsAdapter.getSelectors((state: RootState) => state.studioItems.live)

export const {
  selectAll: selectDeletedStudioItems,
} = studioItemsAdapter.getSelectors((state: RootState) => state.studioItems.deleted)

// Live or deleted, whichever holds it.
export function selectStudioItemById(state: RootState, itemId: string) {
  return state.studioItems.live.entities[itemId] ?? state.studioItems.deleted.entities[itemId]
}
