// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { CodingSession } from '../api/routes/codingSessionRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Redux
import { satelliteDeleted } from './satellitesSlice'

// Newest first, matching the API.
const codingSessionsAdapter = createEntityAdapter<CodingSession>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt),
})

export const codingSessionsSlice = createSlice({
  name: 'codingSessions',
  initialState: codingSessionsAdapter.getInitialState(),
  reducers: {
    codingSessionsLoaded: codingSessionsAdapter.setAll,
    codingSessionUpserted: codingSessionsAdapter.upsertOne,
    codingSessionDeleted(state, action: PayloadAction<number>) {
      codingSessionsAdapter.removeOne(state, action.payload)
    },
  },
  extraReducers: (builder) => {
    builder
      // The API forgets a deleted satellite's sessions without announcing each one.
      .addCase(satelliteDeleted, (state, action) => {
        const orphanedIds: number[] = []
        for (const session of Object.values(state.entities)) {
          if (session.satelliteId === action.payload) {
            orphanedIds.push(session.id)
          }
        }
        codingSessionsAdapter.removeMany(state, orphanedIds)
      })
  },
})

export const { codingSessionsLoaded, codingSessionUpserted, codingSessionDeleted } = codingSessionsSlice.actions

export const {
  selectAll: selectAllCodingSessions,
  selectById: selectCodingSessionById,
} = codingSessionsAdapter.getSelectors((state: RootState) => state.codingSessions)

// The sessions started from one action item, newest first.
export const selectCodingSessionsByActionItemId = createSelector(
  [ selectAllCodingSessions, (_state: RootState, actionItemId: string) => actionItemId ],
  (sessions, actionItemId) => sessions.filter((session) => session.actionItemId === actionItemId),
)

// How many sessions each project holds; projects without one are absent. Pair with
// shallowEqual, since thread state changes replace the session entities constantly.
export const selectSessionCountsByProjectId = createSelector(
  [ selectAllCodingSessions ],
  (sessions) => {
    const countsByProjectId: Record<string, number> = {}
    for (const session of sessions) {
      countsByProjectId[session.projectId] = (countsByProjectId[session.projectId] ?? 0) + 1
    }
    return countsByProjectId
  },
)
