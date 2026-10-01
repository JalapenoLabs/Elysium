// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { WorkspaceSettings } from '../api/routes/userRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type WorkspaceSettingsState = {
  // Admins only load these; null until they have.
  settings: WorkspaceSettings | null
}

const initialState: WorkspaceSettingsState = {
  settings: null,
}

export const workspaceSettingsSlice = createSlice({
  name: 'workspaceSettings',
  initialState,
  reducers: {
    workspaceSettingsUpdated(state, action: PayloadAction<WorkspaceSettings>) {
      state.settings = action.payload
    },
  },
})

export const { workspaceSettingsUpdated } = workspaceSettingsSlice.actions

export function selectWorkspaceSettings(state: RootState) {
  return state.workspaceSettings.settings
}
