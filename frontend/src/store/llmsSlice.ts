// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Llm } from '../api/routes/llmRoutes'
import type { LoadStatus } from './loadStatus'
import type { RootState } from './index'

// Core
import { createAsyncThunk, createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Misc
import { listLlms } from '../api/routes/llmRoutes'

// The order credentials are tried in: priority, then age.
const llmsAdapter = createEntityAdapter<Llm>({
  sortComparer: (first, second) => first.priority - second.priority
    || first.createdAt.localeCompare(second.createdAt),
})

export const fetchLlms = createAsyncThunk('llms/fetch', async () => {
  const response = await listLlms()
  return response.llms
})

export const llmsSlice = createSlice({
  name: 'llms',
  initialState: llmsAdapter.getInitialState({ status: 'idle' as LoadStatus }),
  reducers: {
    llmUpserted: llmsAdapter.upsertOne,
    llmDeleted(state, action: PayloadAction<string>) {
      llmsAdapter.removeOne(state, action.payload)
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchLlms.pending, (state) => {
        if (state.status !== 'loaded') {
          state.status = 'loading'
        }
      })
      .addCase(fetchLlms.fulfilled, (state, action) => {
        llmsAdapter.setAll(state, action.payload)
        state.status = 'loaded'
      })
      .addCase(fetchLlms.rejected, (state, action) => {
        console.debug('Loading LLMs failed', { error: action.error })
        if (state.status !== 'loaded') {
          state.status = 'failed'
        }
      })
  },
})

export const { llmUpserted, llmDeleted } = llmsSlice.actions

export const {
  selectAll: selectAllLlms,
} = llmsAdapter.getSelectors((state: RootState) => state.llms)

export function selectLlmsStatus(state: RootState) {
  return state.llms.status
}
