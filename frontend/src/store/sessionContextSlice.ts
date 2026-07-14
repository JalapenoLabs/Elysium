// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Redux
import { sessionAnonymous, sessionError } from './sessionSlice'

// Serializable projections of `elysium.session.v1.SessionContext`: the accounts
// signed into this browser session and the active account's orgs. Seeded from
// `GET /api/v1/context` and patched live by the realtime stream.

export type SessionAccountView = {
  userId: string
  email: string
  displayName: string
  avatarUrl: string
  isActive: boolean
}

export type SessionOrgView = {
  id: string
  name: string
  slug: string
  role: string
  logoUrl: string
  // White-label branding; empty strings fall back to Elysium defaults.
  accentColor: string
  wordmark: string
}

type SessionContextState = {
  accounts: SessionAccountView[]
  orgs: SessionOrgView[]
  activeOrgId: string | null
  // The realtime high-water mark: inbound envelopes at or below this sequence
  // are already reflected in this state and must be dropped, which is what
  // closes the connect/fetch race (docs/realtime.md).
  asOfSequence: number
  isLoaded: boolean
}

const initialState: SessionContextState = {
  accounts: [],
  orgs: [],
  activeOrgId: null,
  asOfSequence: 0,
  isLoaded: false,
}

type SeededPayload = {
  accounts: SessionAccountView[]
  orgs: SessionOrgView[]
  activeOrgId: string | null
  asOfSequence: number
}

export const sessionContextSlice = createSlice({
  name: 'sessionContext',
  initialState,
  reducers: {
    sessionContextSeeded(state, action: PayloadAction<SeededPayload>) {
      // A stale snapshot (e.g. rebroadcast racing a fresher fetch) never rolls
      // the high-water mark backward.
      if (state.isLoaded && action.payload.asOfSequence < state.asOfSequence) {
        return
      }
      state.accounts = action.payload.accounts
      state.orgs = action.payload.orgs
      state.activeOrgId = action.payload.activeOrgId
      state.asOfSequence = action.payload.asOfSequence
      state.isLoaded = true
    },
    // Advances the high-water mark as realtime envelopes apply.
    realtimeSequenceApplied(state, action: PayloadAction<number>) {
      if (action.payload > state.asOfSequence) {
        state.asOfSequence = action.payload
      }
    },
    // A live `org.updated` push: patch the one org in place.
    orgPatched(
      state,
      action: PayloadAction<{
        id: string
        name: string
        logoUrl: string
        accentColor: string
        wordmark: string
      }>,
    ) {
      const org = state.orgs.find((candidate) => candidate.id === action.payload.id)
      if (org) {
        org.name = action.payload.name
        org.logoUrl = action.payload.logoUrl
        org.accentColor = action.payload.accentColor
        org.wordmark = action.payload.wordmark
      }
    },
  },
  extraReducers: (builder) => {
    builder.addCase(sessionAnonymous, () => initialState)
    builder.addCase(sessionError, () => initialState)
  },
})

export const {
  sessionContextSeeded,
  realtimeSequenceApplied,
  orgPatched,
} = sessionContextSlice.actions
export const sessionContextReducer = sessionContextSlice.reducer
