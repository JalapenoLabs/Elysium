// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { MailServer } from '../api/routes/mailRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type MailServerSliceState = {
  // Null until the first load.
  server: MailServer | null
}

const initialState: MailServerSliceState = {
  server: null,
}

// The one mail server Elysium runs. Replaced whole: every load and every
// `mailServer.updated` event carries its complete status.
export const mailServerSlice = createSlice({
  name: 'mailServer',
  initialState,
  reducers: {
    mailServerUpdated(state, action: PayloadAction<MailServer>) {
      state.server = action.payload
    },
  },
})

export const { mailServerUpdated } = mailServerSlice.actions

export const selectMailServer = (state: RootState) => state.mailServer.server
