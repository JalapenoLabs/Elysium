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
    // A record the API no longer answers for, such as an excerpt deleted with its meeting.
    timelineRecordRemoved: timelineRecordsAdapter.removeOne,
  },
})

export const { timelineRecordsLoaded, timelineRecordUpserted, timelineRecordRemoved } = timelineRecordsSlice.actions

const { selectAll, selectById, selectEntities } = timelineRecordsAdapter.getSelectors(
  (state: RootState) => state.timelineRecords,
)

// One record, when it is loaded, such as a meeting excerpt's, which its lane loads as it opens.
export const selectTimelineRecordById = selectById

// Every record loaded, by id.
export const selectTimelineRecordEntities = selectEntities

// A project's records, newest first: those attributed to it, by a person or by Elysia.
export const selectTimelineRecordsByProjectId = createSelector(
  [ selectAll, (_state: RootState, projectId: string) => projectId ],
  (records, projectId) => records.filter(
    (record) => record.projects.some((attribution) => attribution.projectId === projectId),
  ),
)

// The record that raised an action item, if one did and it is loaded.
export const selectTimelineRecordByActionItemId = createSelector(
  [ selectAll, (_state: RootState, actionItemId: string) => actionItemId ],
  (records, actionItemId) => records.find((record) => record.actionItemId === actionItemId) ?? null,
)
