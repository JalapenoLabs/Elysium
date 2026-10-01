// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Me } from '../api/routes/authRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Redux
import { userUpserted } from './usersSlice'

// Why the API refused a request, by the `code` in its answer. Each sends the person
// somewhere: sign in, the second factor, the waiting page, or setting up an authenticator.
export const ACCESS_REFUSAL_CODES = [
  'unauthenticated',
  'second_factor_required',
  'account_pending',
  'account_disabled',
  'mfa_enrollment_required',
] as const
export type AccessRefusalCode = typeof ACCESS_REFUSAL_CODES[number]

export function isAccessRefusalCode(code: unknown): code is AccessRefusalCode {
  return ACCESS_REFUSAL_CODES.some((known) => known === code)
}

type AuthState = {
  // Who is signed in, once `/api/v1/me` answered.
  me: Me | null
  // The latest refusal no newer answer has cleared. `AuthGate` routes on it.
  refusal: AccessRefusalCode | null
}

const initialState: AuthState = {
  me: null,
  refusal: null,
}

export const authSlice = createSlice({
  name: 'auth',
  initialState,
  reducers: {
    meLoaded(state, action: PayloadAction<Me>) {
      state.me = action.payload
      state.refusal = null
    },
    accessRefused(state, action: PayloadAction<AccessRefusalCode>) {
      state.refusal = action.payload
    },
  },
  extraReducers: (builder) => {
    // An admin changing this person's role or status reaches them through the event stream.
    builder.addCase(userUpserted, (state, action) => {
      if (state.me?.user.id === action.payload.id) {
        state.me.user = action.payload
      }
    })
  },
})

export const { meLoaded, accessRefused } = authSlice.actions

export function selectMe(state: RootState) {
  return state.auth.me
}

export function selectAccessRefusal(state: RootState) {
  return state.auth.refusal
}

export function selectIsAdmin(state: RootState) {
  const user = state.auth.me?.user
  return user?.role === 'admin' && user.status === 'active'
}
