// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { SessionsView } from '../pages/Coding/sessionsView'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Misc
import { readStoredSessionsView } from '../pages/Coding/sessionsView'

type SessionsViewState = {
  view: SessionsView
}

const initialState: SessionsViewState = {
  view: readStoredSessionsView(),
}

export const sessionsViewSlice = createSlice({
  name: 'sessionsView',
  initialState,
  reducers: {
    // A view was chosen, in this tab or (through the storage event) another one.
    // `startSessionsViewSync` saves it.
    sessionsViewChanged(state, action: PayloadAction<SessionsView>) {
      state.view = action.payload
    },
  },
})

export const { sessionsViewChanged } = sessionsViewSlice.actions

export function selectSessionsView(state: RootState) {
  return state.sessionsView.view
}
