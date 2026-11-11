// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
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
    // The list replaces every meeting it could name. It leaves out a meeting merged into another,
    // which only its own page loads, by id, so that one stays: the knowledge base loads the list
    // around every page, and an answer landing after the page's own would otherwise take its
    // meeting away mid-read. A meeting split back out since is in the list again, as its own.
    meetingsLoaded: (state, action: PayloadAction<Meeting[]>) => {
      const listedIds = new Set(action.payload.map((meeting) => meeting.id))
      const mergedAway: Meeting[] = []
      for (const meeting of Object.values(state.entities)) {
        if (meeting?.mergedInto && !listedIds.has(meeting.id)) {
          mergedAway.push(meeting)
        }
      }
      meetingsAdapter.setAll(state, [ ...action.payload, ...mergedAway ])
    },
    meetingUpserted: meetingsAdapter.upsertOne,
  },
})

export const { meetingsLoaded, meetingUpserted } = meetingsSlice.actions

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
