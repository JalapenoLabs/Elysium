// Copyright © 2026 Jalapeno Labs

import type { TimelineRecord } from '../api/routes/timelineRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Newest first, as every timeline shows them; ties fall back to the id, which is time ordered.
const timelineRecordsAdapter = createEntityAdapter<TimelineRecord>({
  sortComparer: (first, second) => second.occurredAt.localeCompare(first.occurredAt)
    || second.id.localeCompare(first.id),
})

// Pages of the timeline merge into one store of records, so paging back and an event's
// reload never drop what another view already holds.
export const timelineRecordsSlice = createSlice({
  name: 'timelineRecords',
  initialState: timelineRecordsAdapter.getInitialState(),
  reducers: {
    timelineRecordsLoaded: timelineRecordsAdapter.upsertMany,
    timelineRecordUpserted: timelineRecordsAdapter.upsertOne,
  },
})

export const { timelineRecordsLoaded, timelineRecordUpserted } = timelineRecordsSlice.actions

const { selectAll } = timelineRecordsAdapter.getSelectors((state: RootState) => state.timelineRecords)

// A project's records, newest first: those attributed to it, by a person or by Elysia.
export const selectTimelineRecordsByProjectId = createSelector(
  [ selectAll, (_state: RootState, projectId: string) => projectId ],
  (records, projectId) => records.filter(
    (record) => record.projects.some((attribution) => attribution.projectId === projectId),
  ),
)

// A meeting's segments in the order they were said, as its page lists its topics.
export const selectTimelineRecordsByMeetingId = createSelector(
  [ selectAll, (_state: RootState, meetingId: string) => meetingId ],
  (records, meetingId) => {
    const segments: TimelineRecord[] = []
    for (const record of records) {
      if (record.media?.meetingId === meetingId) {
        segments.push(record)
      }
    }
    // Every segment has its span, so the earliest start comes first.
    return segments.sort((first, second) => (first.media?.startMs ?? 0) - (second.media?.startMs ?? 0))
  },
)

// The record that raised an action item, if one did and it is loaded.
export const selectTimelineRecordByActionItemId = createSelector(
  [ selectAll, (_state: RootState, actionItemId: string) => actionItemId ],
  (records, actionItemId) => records.find((record) => record.actionItemId === actionItemId) ?? null,
)
