// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { CodingSession } from '../api/routes/codingSessionRoutes'
import type { LoadStatus } from './loadStatus'
import type { RootState } from './index'

// Core
import { createAsyncThunk, createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Redux
import { satelliteDeleted } from './satellitesSlice'

// Misc
import { listCodingSessions } from '../api/routes/codingSessionRoutes'

// Newest first, matching the API.
const codingSessionsAdapter = createEntityAdapter<CodingSession>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt),
})

export const fetchCodingSessions = createAsyncThunk('codingSessions/fetch', async () => {
  const response = await listCodingSessions()
  return response.sessions
})

export const codingSessionsSlice = createSlice({
  name: 'codingSessions',
  initialState: codingSessionsAdapter.getInitialState({ status: 'idle' as LoadStatus }),
  reducers: {
    codingSessionUpserted: codingSessionsAdapter.upsertOne,
    codingSessionDeleted(state, action: PayloadAction<string>) {
      codingSessionsAdapter.removeOne(state, action.payload)
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchCodingSessions.pending, (state) => {
        if (state.status !== 'loaded') {
          state.status = 'loading'
        }
      })
      .addCase(fetchCodingSessions.fulfilled, (state, action) => {
        codingSessionsAdapter.setAll(state, action.payload)
        state.status = 'loaded'
      })
      .addCase(fetchCodingSessions.rejected, (state, action) => {
        console.debug('Loading coding sessions failed', { error: action.error })
        if (state.status !== 'loaded') {
          state.status = 'failed'
        }
      })
      // The API forgets a deleted satellite's sessions without announcing each one.
      .addCase(satelliteDeleted, (state, action) => {
        const orphanedIds: string[] = []
        for (const session of Object.values(state.entities)) {
          if (session.satelliteId === action.payload) {
            orphanedIds.push(session.id)
          }
        }
        codingSessionsAdapter.removeMany(state, orphanedIds)
      })
  },
})

export const { codingSessionUpserted, codingSessionDeleted } = codingSessionsSlice.actions

export const {
  selectAll: selectAllCodingSessions,
  selectById: selectCodingSessionById,
} = codingSessionsAdapter.getSelectors((state: RootState) => state.codingSessions)

export function selectCodingSessionsStatus(state: RootState) {
  return state.codingSessions.status
}
