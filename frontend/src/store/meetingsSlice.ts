// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../api/routes/meetingRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Newest first, as the API lists them: the later id first between two that started together, so
// two never tie, in the Meetings list or as siblings in the tree.
const meetingsAdapter = createEntityAdapter<Meeting>({
  sortComparer: (first, second) => second.startedAt.localeCompare(first.startedAt)
    || second.id.localeCompare(first.id),
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
  selectById: selectMeetingById,
} = meetingsAdapter.getSelectors((state: RootState) => state.meetings)
