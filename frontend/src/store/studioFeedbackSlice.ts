// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { StudioFeedback } from '../api/routes/studioRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Oldest first, as a conversation reads. The id breaks ties between prompts sent in the
// same instant.
const studioFeedbackAdapter = createEntityAdapter<StudioFeedback>({
  sortComparer: (first, second) => first.createdAt.localeCompare(second.createdAt)
    || first.id.localeCompare(second.id),
})

// The drawn prompts of the items viewed.
export const studioFeedbackSlice = createSlice({
  name: 'studioFeedback',
  initialState: studioFeedbackAdapter.getInitialState(),
  reducers: {
    // Replaces one item's feedback.
    studioFeedbackLoaded(state, action: PayloadAction<{ studioItemId: string, feedback: StudioFeedback[] }>) {
      const staleIds: string[] = []
      for (const feedback of Object.values(state.entities)) {
        if (feedback.studioItemId === action.payload.studioItemId) {
          staleIds.push(feedback.id)
        }
      }
      studioFeedbackAdapter.removeMany(state, staleIds)
      studioFeedbackAdapter.setMany(state, action.payload.feedback)
    },
    studioFeedbackCreated: studioFeedbackAdapter.setOne,
  },
})

export const { studioFeedbackLoaded, studioFeedbackCreated } = studioFeedbackSlice.actions

const { selectAll: selectAllStudioFeedback } = studioFeedbackAdapter.getSelectors(
  (state: RootState) => state.studioFeedback,
)

// One item's drawn prompts, oldest first.
export const selectStudioItemFeedback = createSelector(
  [ selectAllStudioFeedback, (_state: RootState, studioItemId: string) => studioItemId ],
  (feedback, studioItemId) => feedback.filter((entry) => entry.studioItemId === studioItemId),
)
