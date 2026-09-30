// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { CodingSession } from '../api/routes/codingSessionRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Newest first, matching the API.
const codingSessionsAdapter = createEntityAdapter<CodingSession>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt),
})

// Coding and Studio sessions share one slice, since both arrive through the same session
// events. They load apart, though: the Coding list answers only Coding sessions, and
// Studio's arrive with their items, so each load leaves the other kind in place.
export const codingSessionsSlice = createSlice({
  name: 'codingSessions',
  initialState: codingSessionsAdapter.getInitialState(),
  reducers: {
    // Replaces the Coding sessions, so one deleted while the page was away goes too.
    codingSessionsLoaded(state, action: PayloadAction<CodingSession[]>) {
      const staleIds: number[] = []
      for (const session of Object.values(state.entities)) {
        if (session.studioItemId === null) {
          staleIds.push(session.id)
        }
      }
      codingSessionsAdapter.removeMany(state, staleIds)
      codingSessionsAdapter.setMany(state, action.payload)
    },
    // Sessions that arrive with Studio items, which never replace any others.
    codingSessionsUpserted: codingSessionsAdapter.upsertMany,
    codingSessionUpserted: codingSessionsAdapter.upsertOne,
    codingSessionDeleted(state, action: PayloadAction<number>) {
      codingSessionsAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  codingSessionsLoaded,
  codingSessionsUpserted,
  codingSessionUpserted,
  codingSessionDeleted,
} = codingSessionsSlice.actions

// Every session of either kind. Views list one kind, through the selectors below.
const {
  selectAll: selectAllSessions,
  selectById: selectSessionById,
} = codingSessionsAdapter.getSelectors((state: RootState) => state.codingSessions)

// One Coding session; a Studio session's number answers undefined, since the Coding page
// never shows one.
export function selectCodingSessionById(state: RootState, sessionId: number) {
  const session = selectSessionById(state, sessionId)
  if (session?.studioItemId) {
    return undefined
  }
  return session
}

// The Coding page's sessions, newest first. Every Coding view lists these, never Studio's.
export const selectCodingSessions = createSelector(
  [ selectAllSessions ],
  (sessions) => sessions.filter((session) => session.studioItemId === null),
)

// The Coding sessions started from one action item, newest first.
export const selectCodingSessionsByActionItemId = createSelector(
  [ selectCodingSessions, (_state: RootState, actionItemId: string) => actionItemId ],
  (sessions, actionItemId) => sessions.filter((session) => session.actionItemId === actionItemId),
)

// The Coding sessions of one project, newest first.
export const selectCodingSessionsByProjectId = createSelector(
  [ selectCodingSessions, (_state: RootState, projectId: string) => projectId ],
  (sessions, projectId) => sessions.filter((session) => session.projectId === projectId),
)

// How many Coding sessions each project holds; projects without one are absent. Pair with
// shallowEqual, since thread state changes replace the session entities constantly.
export const selectSessionCountsByProjectId = createSelector(
  [ selectCodingSessions ],
  (sessions) => {
    const countsByProjectId: Record<string, number> = {}
    for (const session of sessions) {
      if (session.projectId) {
        countsByProjectId[session.projectId] = (countsByProjectId[session.projectId] ?? 0) + 1
      }
    }
    return countsByProjectId
  },
)

// One Studio item's sessions, oldest first: the order its conversation reads in.
export const selectStudioItemSessions = createSelector(
  [ selectAllSessions, (_state: RootState, studioItemId: string) => studioItemId ],
  (sessions, studioItemId) => {
    const itemSessions: CodingSession[] = []
    // The adapter holds them newest first; walking backwards reads them oldest first.
    for (let index = sessions.length - 1; index >= 0; index--) {
      if (sessions[index].studioItemId === studioItemId) {
        itemSessions.push(sessions[index])
      }
    }
    return itemSessions
  },
)

// Each Studio item's latest session, for the grid's live thread state. Items with no
// session loaded are absent.
export const selectLatestSessionsByStudioItemId = createSelector(
  [ selectAllSessions ],
  (sessions) => {
    const latestByItemId: Record<string, CodingSession> = {}
    for (const session of sessions) {
      // Newest first, so the first one seen per item is its latest.
      if (session.studioItemId && !latestByItemId[session.studioItemId]) {
        latestByItemId[session.studioItemId] = session
      }
    }
    return latestByItemId
  },
)
