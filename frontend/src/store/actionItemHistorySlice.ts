// Copyright © 2026 Jalapeno Labs

import type { HistoryEntry } from '../api/routes/actionItemRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Oldest first, as the API answers. The id breaks ties between entries one write recorded.
const historyAdapter = createEntityAdapter<HistoryEntry>({
  sortComparer: (first, second) => first.createdAt.localeCompare(second.createdAt)
    || first.id.localeCompare(second.id),
})

// Item and initiative histories in one collection, since an item joining or leaving an
// initiative is one entry that belongs to both. History is append-only, so a load merges
// by id rather than replacing, and an entry that arrives live while it loads is kept.
export const actionItemHistorySlice = createSlice({
  name: 'actionItemHistory',
  initialState: historyAdapter.getInitialState(),
  reducers: {
    historyLoaded: historyAdapter.setMany,
    historyAppended: historyAdapter.setOne,
  },
})

export const { historyLoaded, historyAppended } = actionItemHistorySlice.actions

const { selectAll: selectAllHistory } = historyAdapter.getSelectors(
  (state: RootState) => state.actionItemHistory,
)

export const selectActionItemHistory = createSelector(
  [ selectAllHistory, (_state: RootState, actionItemId: string) => actionItemId ],
  (entries, actionItemId) => entries.filter((entry) => entry.actionItemId === actionItemId),
)

export const selectInitiativeHistory = createSelector(
  [ selectAllHistory, (_state: RootState, initiativeId: string) => initiativeId ],
  (entries, initiativeId) => entries.filter((entry) => entry.initiativeId === initiativeId),
)
