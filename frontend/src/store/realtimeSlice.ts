// Copyright © 2026 Jalapeno Labs

import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

// `connecting` is the first attempt; `reconnecting` means updates stopped arriving
// and what is on screen may be stale until the stream is back.
export type EventStreamConnection = 'connecting' | 'open' | 'reconnecting'

type RealtimeState = {
  connection: EventStreamConnection
}

const initialState: RealtimeState = {
  connection: 'connecting',
}

export const realtimeSlice = createSlice({
  name: 'realtime',
  initialState,
  reducers: {
    eventStreamOpened(state) {
      state.connection = 'open'
    },
    eventStreamLost(state) {
      state.connection = 'reconnecting'
    },
  },
})

export const { eventStreamOpened, eventStreamLost } = realtimeSlice.actions

export function selectEventStreamConnection(state: RootState) {
  return state.realtime.connection
}
