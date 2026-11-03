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
