// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { Learning } from '../api/routes/learningRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Most recently changed first, as the API lists them.
const learningsAdapter = createEntityAdapter<Learning>({
  sortComparer: (first, second) => second.updatedAt.localeCompare(first.updatedAt),
})

// Live and deleted learnings are held apart because they load apart, as action items are: each
// list answers one or the other, and each load replaces only its own half.
const initialState = {
  live: learningsAdapter.getInitialState(),
  deleted: learningsAdapter.getInitialState(),
}

export const learningsSlice = createSlice({
  name: 'learnings',
  initialState,
  reducers: {
    learningsLoaded(state, action: PayloadAction<Learning[]>) {
      learningsAdapter.setAll(state.live, action.payload)
    },
    deletedLearningsLoaded(state, action: PayloadAction<Learning[]>) {
      learningsAdapter.setAll(state.deleted, action.payload)
    },
    // Files the learning under live or deleted by its `deletedAt`, so a delete or a restore
    // moves it across.
    learningUpserted(state, action: PayloadAction<Learning>) {
      const learning = action.payload
      if (learning.deletedAt) {
        learningsAdapter.removeOne(state.live, learning.id)
        learningsAdapter.setOne(state.deleted, learning)
        return
      }
      learningsAdapter.removeOne(state.deleted, learning.id)
      learningsAdapter.setOne(state.live, learning)
    },
    // A learning the API no longer answers for.
    learningRemoved(state, action: PayloadAction<string>) {
      learningsAdapter.removeOne(state.live, action.payload)
      learningsAdapter.removeOne(state.deleted, action.payload)
    },
  },
})

export const {
  learningsLoaded,
  deletedLearningsLoaded,
  learningUpserted,
  learningRemoved,
} = learningsSlice.actions

export const {
  selectAll: selectLiveLearnings,
  selectById: selectLiveLearningById,
} = learningsAdapter.getSelectors((state: RootState) => state.learnings.live)

export const {
  selectAll: selectDeletedLearnings,
  selectById: selectDeletedLearningById,
} = learningsAdapter.getSelectors((state: RootState) => state.learnings.deleted)

// The learning with this id, live or deleted.
export function selectLearningById(state: RootState, learningId: string) {
  return selectLiveLearningById(state, learningId) ?? selectDeletedLearningById(state, learningId)
}
