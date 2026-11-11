// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../api/routes/meetingRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

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
  selectById: selectMeetingById,
} = meetingsAdapter.getSelectors((state: RootState) => state.meetings)

// The meetings a list shows: every one but those merged into another, which live on in it and
// are listed nowhere (docs/meetings.md, Meetings and captures). A merged meeting still reaches
// the store, from its own page or from the event a merge announces, and stays there for its page.
export const selectListedMeetings = createSelector(
  [ selectAllMeetings ],
  (meetings) => meetings.filter((meeting) => !meeting.mergedInto),
)
