// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { UpdatesStatus } from '../api/routes/updateRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type UpdatesSliceState = {
  // Admins load it on the Updates page; everyone receives its `updates.status` events, which
  // also name the build running now (`versionSlice`). Null until either arrives.
  status: UpdatesStatus | null
}

const initialState: UpdatesSliceState = {
  status: null,
}

// Auto-update's state. Replaced whole: every load and every `updates.status` event carries
// the complete status.
export const updatesSlice = createSlice({
  name: 'updates',
  initialState,
  reducers: {
    updatesStatusUpdated(state, action: PayloadAction<UpdatesStatus>) {
      state.status = action.payload
    },
  },
})

export const { updatesStatusUpdated } = updatesSlice.actions

export function selectUpdatesStatus(state: RootState) {
  return state.updates.status
}
