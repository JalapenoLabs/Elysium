// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'

// Core
import { createSlice } from '@reduxjs/toolkit'

// The lifecycle of the browser's knowledge of its own session.
//   unknown       - not resolved yet (first paint)
//   loading       - a resolve is in flight
//   authenticated - a valid session cookie is present
//   anonymous     - no session; the user must sign in
//   error         - the identity service could not be reached
export type SessionStatus = 'unknown' | 'loading' | 'authenticated' | 'anonymous' | 'error'

// The principal projected from the API's session, mirroring `elysium.auth.v1.MeResponse`.
export type Principal = {
  subject: string
  email: string
  username: string
}

type SessionState = {
  status: SessionStatus
  principal: Principal | null
  // Whether this instance has been claimed by its root user. `null` until the
  // first-run check resolves; `false` routes the app into first-time setup.
  instanceClaimed: boolean | null
}

const initialState: SessionState = {
  status: 'unknown',
  principal: null,
  instanceClaimed: null,
}

export const sessionSlice = createSlice({
  name: 'session',
  initialState,
  reducers: {
    sessionLoading(state) {
      state.status = 'loading'
    },
    sessionAuthenticated(state, action: PayloadAction<Principal>) {
      state.status = 'authenticated'
      state.principal = action.payload
    },
    sessionAnonymous(state) {
      state.status = 'anonymous'
      state.principal = null
    },
    sessionError(state) {
      state.status = 'error'
      state.principal = null
    },
    instanceClaimedResolved(state, action: PayloadAction<boolean>) {
      state.instanceClaimed = action.payload
    },
  },
})

export const {
  sessionLoading,
  sessionAuthenticated,
  sessionAnonymous,
  sessionError,
  instanceClaimedResolved,
} = sessionSlice.actions
export const sessionReducer = sessionSlice.reducer
