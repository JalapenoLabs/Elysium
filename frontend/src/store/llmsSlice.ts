// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Llm } from '../api/routes/llmRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// The order credentials are tried in: priority, then age.
const llmsAdapter = createEntityAdapter<Llm>({
  sortComparer: (first, second) => first.priority - second.priority
    || first.createdAt.localeCompare(second.createdAt),
})

export const llmsSlice = createSlice({
  name: 'llms',
  initialState: llmsAdapter.getInitialState(),
  reducers: {
    llmsLoaded: llmsAdapter.setAll,
    llmUpserted: llmsAdapter.upsertOne,
    llmDeleted(state, action: PayloadAction<string>) {
      llmsAdapter.removeOne(state, action.payload)
    },
  },
})

export const { llmsLoaded, llmUpserted, llmDeleted } = llmsSlice.actions

export const {
  selectAll: selectAllLlms,
} = llmsAdapter.getSelectors((state: RootState) => state.llms)
