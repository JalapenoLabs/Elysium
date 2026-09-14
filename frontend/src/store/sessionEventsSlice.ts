// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { SessionEvent } from '../api/routes/codingSessionRoutes'
import type { LoadStatus } from './loadStatus'
import type { RootState } from './index'

// Core
import { createAsyncThunk, createSlice } from '@reduxjs/toolkit'

// Redux
import { codingSessionDeleted } from './codingSessionsSlice'

// Misc
import { listSessionEvents } from '../api/routes/codingSessionRoutes'
import { SESSION_EVENTS_LIMIT } from '../constants'

// A conversation's events, kept only for sessions someone has opened. Live events for
// other sessions are dropped: nothing shows them, and history is fetched on opening.
export type SessionTimeline = {
  status: LoadStatus
  // Sorted by sequence, without duplicates.
  events: SessionEvent[]
  // Older events exist on the satellite but were not loaded.
  truncated: boolean
}

type SessionEventsState = {
  bySessionId: Record<string, SessionTimeline>
}

const initialState: SessionEventsState = {
  bySessionId: {},
}

export const fetchSessionEvents = createAsyncThunk(
  'sessionEvents/fetch',
  async (sessionId: string) => {
    const response = await listSessionEvents(sessionId)
    return response
  },
)

export const sessionEventsSlice = createSlice({
  name: 'sessionEvents',
  initialState,
  reducers: {
    sessionEventReceived(state, action: PayloadAction<SessionEvent>) {
      const timeline = state.bySessionId[action.payload.sessionId]
      if (!timeline) {
        return
      }
      mergeEvents(timeline, [ action.payload ])
    },
    // A conversation panel closed; its events are not worth keeping in memory.
    sessionTimelineReleased(state, action: PayloadAction<string>) {
      delete state.bySessionId[action.payload]
    },
  },
  extraReducers: (builder) => {
    builder
      .addCase(fetchSessionEvents.pending, (state, action) => {
        const sessionId = action.meta.arg
        const timeline = state.bySessionId[sessionId]
        if (!timeline) {
          // Live events that arrive while history loads are kept and merged with it.
          state.bySessionId[sessionId] = { status: 'loading', events: [], truncated: false }
          return
        }
        if (timeline.status !== 'loaded') {
          timeline.status = 'loading'
        }
      })
      .addCase(fetchSessionEvents.fulfilled, (state, action) => {
        const timeline = state.bySessionId[action.meta.arg]
        if (!timeline) {
          console.debug('History arrived for a released timeline', { sessionId: action.meta.arg })
          return
        }
        mergeEvents(timeline, action.payload.events)
        timeline.truncated = action.payload.truncated
        timeline.status = 'loaded'
      })
      .addCase(fetchSessionEvents.rejected, (state, action) => {
        console.debug('Loading session history failed', { error: action.error, sessionId: action.meta.arg })
        const timeline = state.bySessionId[action.meta.arg]
        if (timeline && timeline.status !== 'loaded') {
          timeline.status = 'failed'
        }
      })
      .addCase(codingSessionDeleted, (state, action) => {
        delete state.bySessionId[action.payload]
      })
  },
})

// Merges sorted `incoming` events into a timeline, keeping it sorted by sequence and
// free of duplicates. Live events almost always arrive after the last one held, so
// that case appends; anything else (history landing under live events) is one
// two-pointer pass over both lists.
function mergeEvents(timeline: SessionTimeline, incoming: SessionEvent[]) {
  const existing = timeline.events
  const last = existing.at(-1)

  if (!last || !incoming.length || last.sequence < incoming[0].sequence) {
    existing.push(...incoming)
  }
  else {
    const merged: SessionEvent[] = []
    let existingIndex = 0
    let incomingIndex = 0

    while (existingIndex < existing.length || incomingIndex < incoming.length) {
      const held = existing[existingIndex]
      const arriving = incoming[incomingIndex]

      if (!arriving || (held && held.sequence < arriving.sequence)) {
        merged.push(held)
        existingIndex++
        continue
      }
      if (!held || arriving.sequence < held.sequence) {
        merged.push(arriving)
        incomingIndex++
        continue
      }

      // The same event from both sources.
      merged.push(held)
      existingIndex++
      incomingIndex++
    }
    timeline.events = merged
  }

  const overflow = timeline.events.length - SESSION_EVENTS_LIMIT
  if (overflow > 0) {
    timeline.events.splice(0, overflow)
    timeline.truncated = true
  }
}

export const { sessionEventReceived, sessionTimelineReleased } = sessionEventsSlice.actions

export function selectSessionTimeline(state: RootState, sessionId: string) {
  return state.sessionEvents.bySessionId[sessionId]
}
