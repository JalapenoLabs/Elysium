// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Draft } from '../api/routes/draftRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Newest first, as a record's reply box shows them; ties fall back to the id, which is time
// ordered.
const draftsAdapter = createEntityAdapter<Draft>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt)
    || second.id.localeCompare(first.id),
})

type RecordDraftsLoaded = {
  recordId: string
  drafts: Draft[]
}

// The person's drafts of the records viewed. A record's load replaces what is held for it, so
// a draft discarded in another tab does not linger.
export const draftsSlice = createSlice({
  name: 'drafts',
  initialState: draftsAdapter.getInitialState(),
  reducers: {
    recordDraftsLoaded: (state, action: PayloadAction<RecordDraftsLoaded>) => {
      const stale: string[] = []
      for (const draft of Object.values(state.entities)) {
        if (draft.replyToRecordId === action.payload.recordId) {
          stale.push(draft.id)
        }
      }
      draftsAdapter.removeMany(state, stale)
      draftsAdapter.upsertMany(state, action.payload.drafts)
    },
    draftUpserted: draftsAdapter.upsertOne,
    draftRemoved: draftsAdapter.removeOne,
  },
})

export const { recordDraftsLoaded, draftUpserted, draftRemoved } = draftsSlice.actions

const { selectAll } = draftsAdapter.getSelectors((state: RootState) => state.drafts)

// A record's drafts that were never sent, newest first: the one its reply box continues. A
// follow-up drafted for a promise answers the record too, but belongs to its item's page.
export const selectUnsentRecordDrafts = createSelector(
  [ selectAll, (_state: RootState, recordId: string) => recordId ],
  (drafts, recordId) => drafts.filter(
    (draft) => draft.replyToRecordId === recordId && draft.state !== 'sent' && !draft.promiseId,
  ),
)

// One draft by id, once loaded; `undefined` until then, or when it is not the person's.
export const { selectById: selectDraftById } = draftsAdapter.getSelectors((state: RootState) => state.drafts)
