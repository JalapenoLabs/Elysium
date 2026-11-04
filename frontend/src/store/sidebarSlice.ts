// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Misc
import { readStoredSidebarCollapsed } from '../layout/sidebarCollapse'

type SidebarState = {
  // Collapsed to its icons, from md up; the phone drawer always shows the labels.
  isCollapsed: boolean
}

const initialState: SidebarState = {
  isCollapsed: readStoredSidebarCollapsed(),
}

export const sidebarSlice = createSlice({
  name: 'sidebar',
  initialState,
  reducers: {
    // The sidebar was collapsed or expanded, in this tab or (through the storage event)
    // another one. `startSidebarSync` saves it.
    sidebarCollapsedChanged(state, action: PayloadAction<boolean>) {
      state.isCollapsed = action.payload
    },
  },
})

export const { sidebarCollapsedChanged } = sidebarSlice.actions

export function selectIsSidebarCollapsed(state: RootState) {
  return state.sidebar.isCollapsed
}
