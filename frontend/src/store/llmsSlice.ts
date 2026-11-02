// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Llm, LlmUsage } from '../api/routes/llmRoutes'
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
    // Only an account has usage; an event for a credential not loaded yet is left to the
    // next load, which carries it.
    llmUsageUpdated(state, action: PayloadAction<{ id: string, usage: LlmUsage }>) {
      const account = state.entities[action.payload.id]?.account
      if (!account) {
        console.debug('llmUsageUpdated has no account to update', { id: action.payload.id })
        return
      }
      account.usage = action.payload.usage
    },
  },
})

export const { llmsLoaded, llmUpserted, llmDeleted, llmUsageUpdated } = llmsSlice.actions

export const {
  selectAll: selectAllLlms,
  selectById: selectLlmById,
} = llmsAdapter.getSelectors((state: RootState) => state.llms)
