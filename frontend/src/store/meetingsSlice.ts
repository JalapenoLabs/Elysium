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

// One store of every meeting the person has loaded: those listed, those merged into another, and
// those deleted. Each listing replaces only what it could name, and the selectors below say which
// a view shows, so a meeting moves between them as an event loads it again.
export const meetingsSlice = createSlice({
  name: 'meetings',
  initialState: meetingsAdapter.getInitialState(),
  reducers: {
    // The list replaces every meeting it could name. It leaves out a meeting merged into another
    // and one deleted, which their own page loads by id (and the Deleted filter lists), so those
    // stay: the knowledge base loads the list around every page, and an answer landing after the
    // page's own would otherwise take its meeting away mid-read. A meeting split back out or
    // restored since is in the list again, as its own.
    meetingsLoaded: (state, action: PayloadAction<Meeting[]>) => {
      const listedIds = new Set(action.payload.map((meeting) => meeting.id))
      const unlisted: Meeting[] = []
      for (const meeting of Object.values(state.entities)) {
        if (!meeting || listedIds.has(meeting.id)) {
          continue
        }
        if (meeting.mergedInto || meeting.deletedAt) {
          unlisted.push(meeting)
        }
      }
      meetingsAdapter.setAll(state, [ ...action.payload, ...unlisted ])
    },
    // The deleted list replaces every deleted meeting the store holds, keeping the rest.
    deletedMeetingsLoaded: (state, action: PayloadAction<Meeting[]>) => {
      const deletedIds = new Set(action.payload.map((meeting) => meeting.id))
      const stale: string[] = []
      for (const meeting of Object.values(state.entities)) {
        if (meeting?.deletedAt && !deletedIds.has(meeting.id)) {
          stale.push(meeting.id)
        }
      }
      meetingsAdapter.removeMany(state, stale)
      meetingsAdapter.upsertMany(state, action.payload)
    },
    meetingUpserted: meetingsAdapter.upsertOne,
    // The meetings one change to a selection answered with, filed in one update.
    meetingsUpserted: meetingsAdapter.upsertMany,
  },
})

export const {
  meetingsLoaded,
  deletedMeetingsLoaded,
  meetingUpserted,
  meetingsUpserted,
} = meetingsSlice.actions

export const {
  selectAll: selectAllMeetings,
  selectById: selectMeetingById,
} = meetingsAdapter.getSelectors((state: RootState) => state.meetings)

// The meetings every list shows: all but those merged into another, which live on in it, and those
// deleted. An event loads either by id, which keeps it in the store for its own page, so the lists
// ask here.
export const selectListedMeetings = createSelector(
  [ selectAllMeetings ],
  (meetings) => meetings.filter((meeting) => meeting.mergedInto === null && meeting.deletedAt === null),
)

// The person's deleted meetings, as the Meetings page's Deleted filter lists them.
export const selectDeletedMeetings = createSelector(
  [ selectAllMeetings ],
  (meetings) => meetings.filter((meeting) => meeting.deletedAt !== null),
)
