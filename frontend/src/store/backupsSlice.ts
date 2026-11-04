// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { BackupsStatus } from '../api/routes/backupRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type BackupsSliceState = {
  // Admins only load it; null until they have.
  status: BackupsStatus | null
}

const initialState: BackupsSliceState = {
  status: null,
}

// The database backups. Replaced whole: every load and every `backups.status` event carries
// the complete status.
export const backupsSlice = createSlice({
  name: 'backups',
  initialState,
  reducers: {
    backupsStatusUpdated(state, action: PayloadAction<BackupsStatus>) {
      state.status = action.payload
    },
  },
})

export const { backupsStatusUpdated } = backupsSlice.actions

export function selectBackupsStatus(state: RootState) {
  return state.backups.status
}
