// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { BrowserSession } from '../api/routes/browserSessionRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

const browserSessionsAdapter = createEntityAdapter<BrowserSession>({
  sortComparer: (first, second) => first.createdAt.localeCompare(second.createdAt),
})

export const browserSessionsSlice = createSlice({
  name: 'browserSessions',
  initialState: browserSessionsAdapter.getInitialState(),
  reducers: {
    browserSessionsLoaded: browserSessionsAdapter.setAll,
    browserSessionUpserted: browserSessionsAdapter.upsertOne,
    browserSessionDeleted(state, action: PayloadAction<string>) {
      browserSessionsAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  browserSessionsLoaded,
  browserSessionUpserted,
  browserSessionDeleted,
} = browserSessionsSlice.actions

export const {
  selectAll: selectAllBrowserSessions,
  selectById: selectBrowserSessionById,
} = browserSessionsAdapter.getSelectors((state: RootState) => state.browserSessions)
