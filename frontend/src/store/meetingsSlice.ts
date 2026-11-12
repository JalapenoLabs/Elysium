// Copyright © 2026 Jalapeno Labs

import type { Meeting } from '../api/routes/meetingRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

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
    // A meeting the person may no longer read, such as one made private, leaves every list.
    meetingRemoved: meetingsAdapter.removeOne,
  },
})

export const { meetingsLoaded, meetingUpserted, meetingRemoved } = meetingsSlice.actions

export const {
  selectAll: selectAllMeetings,
  selectById: selectMeetingById,
} = meetingsAdapter.getSelectors((state: RootState) => state.meetings)

// The meetings every list shows: all but those merged into another, which live on in it. An event
// loads a merged one by id, which keeps it in the store for its own page, so the lists ask here.
export const selectListedMeetings = createSelector(
  [ selectAllMeetings ],
  (meetings) => meetings.filter((meeting) => meeting.mergedInto === null),
)
