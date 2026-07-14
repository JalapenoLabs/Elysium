// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ManualStatusKind, Presence, User } from '../proto/generated/user_pb'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Redux
import { sessionAnonymous, sessionAuthenticated, sessionError } from './sessionSlice'

// A serializable projection of `elysium.user.v1.User` for the signed-in user.
// The generated proto message carries bigint timestamps, which Redux can't
// store; every producer projects through `toCurrentUser` before dispatching.
export type CurrentUser = {
  id: string
  subject: string
  email: string
  givenName: string
  familyName: string
  displayName: string
  avatarUrl: string
  bio: string
  presence: Presence
  manualStatus: ManualStatusKind
  manualStatusText: string
}

// The one projection from the wire shape into the stored shape, shared by the
// API routes and the realtime stream.
export function toCurrentUser(user: User): CurrentUser {
  return {
    id: user.id?.value ?? '',
    subject: user.subject,
    email: user.email,
    givenName: user.givenName,
    familyName: user.familyName,
    displayName: user.displayName,
    avatarUrl: user.avatarUrl,
    bio: user.bio,
    presence: user.presence,
    manualStatus: user.manualStatus,
    manualStatusText: user.manualStatusText,
  }
}

type CurrentUserState = {
  // `null` until the first profile fetch after authentication resolves.
  user: CurrentUser | null
}

const initialState: CurrentUserState = {
  user: null,
}

export const currentUserSlice = createSlice({
  name: 'currentUser',
  initialState,
  reducers: {
    currentUserLoaded(state, action: PayloadAction<CurrentUser>) {
      state.user = action.payload
    },
    // Live presence pushes only carry the new presence; the rest of the
    // profile stays as loaded.
    currentUserPresenceChanged(state, action: PayloadAction<Presence>) {
      if (state.user) {
        state.user.presence = action.payload
      }
    },
  },
  // The profile is meaningless without a session, so it clears itself whenever
  // the session ends, and whenever the session's principal changes to a
  // different subject (an account switch); consumers then refetch for the new
  // active account. No caller has to remember to do any of this.
  extraReducers: (builder) => {
    builder.addCase(sessionAnonymous, (state) => {
      state.user = null
    })
    builder.addCase(sessionError, (state) => {
      state.user = null
    })
    builder.addCase(sessionAuthenticated, (state, action) => {
      if (state.user && state.user.subject !== action.payload.subject) {
        state.user = null
      }
    })
  },
})

export const { currentUserLoaded, currentUserPresenceChanged } = currentUserSlice.actions
export const currentUserReducer = currentUserSlice.reducer
