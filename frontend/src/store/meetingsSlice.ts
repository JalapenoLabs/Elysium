// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../api/routes/meetingRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Newest first, as the meetings list shows them.
const meetingsAdapter = createEntityAdapter<Meeting>({
  sortComparer: (first, second) => second.startedAt.localeCompare(first.startedAt),
})

export const meetingsSlice = createSlice({
  name: 'meetings',
  initialState: meetingsAdapter.getInitialState(),
  reducers: {
    meetingsLoaded: meetingsAdapter.setAll,
    meetingUpserted: meetingsAdapter.upsertOne,
  },
})

export const { meetingsLoaded, meetingUpserted } = meetingsSlice.actions

export const {
  selectAll: selectAllMeetings,
} = meetingsAdapter.getSelectors((state: RootState) => state.meetings)
