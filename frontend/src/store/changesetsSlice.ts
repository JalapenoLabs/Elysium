// Copyright © 2026 Jalapeno Labs

import type { Changeset } from '../api/routes/changesetRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Newest first. Ids are UUIDv7, so they order changesets the way they were proposed.
const changesetsAdapter = createEntityAdapter<Changeset>({
  sortComparer: (first, second) => second.id.localeCompare(first.id),
})

export const changesetsSlice = createSlice({
  name: 'changesets',
  initialState: changesetsAdapter.getInitialState(),
  reducers: {
    changesetsLoaded: changesetsAdapter.setAll,
    changesetUpserted: changesetsAdapter.setOne,
  },
})

export const { changesetsLoaded, changesetUpserted } = changesetsSlice.actions

const {
  selectAll: selectAllChangesets,
  selectById,
} = changesetsAdapter.getSelectors((state: RootState) => state.changesets)

export const selectChangesets = selectAllChangesets

export function selectChangesetById(state: RootState, changesetId: string) {
  return selectById(state, changesetId)
}

// Waiting for the user's review, newest first.
export const selectPendingChangesets = createSelector(
  [ selectAllChangesets ],
  (changesets) => changesets.filter((changeset) => changeset.state === 'pending'),
)
