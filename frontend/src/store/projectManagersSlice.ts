// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ProjectManager } from '../api/routes/projectManagerRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type ProjectManagersState = {
  // Each project's Project Manager its page has loaded, replaced whole by every load.
  byProject: Record<string, ProjectManager>
  // The turn logs a page has opened, by turn.
  logsByTurn: Record<string, string>
}

const initialState: ProjectManagersState = {
  byProject: {},
  logsByTurn: {},
}

export const projectManagersSlice = createSlice({
  name: 'projectManagers',
  initialState,
  reducers: {
    projectManagerLoaded(state, action: PayloadAction<ProjectManager>) {
      state.byProject[action.payload.projectId] = action.payload
    },
    turnLogLoaded(state, action: PayloadAction<{ turnId: string, log: string }>) {
      state.logsByTurn[action.payload.turnId] = action.payload.log
    },
  },
})

export const { projectManagerLoaded, turnLogLoaded } = projectManagersSlice.actions

// Project `projectId`'s Project Manager once its page loaded it; `undefined` until then.
export function selectProjectManager(state: RootState, projectId: string) {
  return state.projectManagers.byProject[projectId]
}

// A turn's log once a page opened it; `undefined` until then.
export function selectTurnLog(state: RootState, turnId: string) {
  return state.projectManagers.logsByTurn[turnId]
}
