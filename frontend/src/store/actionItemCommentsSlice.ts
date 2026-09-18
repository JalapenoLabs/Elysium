// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ActionItemComment } from '../api/routes/actionItemRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Oldest first, as a conversation reads. The id breaks ties between comments written in
// the same instant.
const commentsAdapter = createEntityAdapter<ActionItemComment>({
  sortComparer: (first, second) => first.createdAt.localeCompare(second.createdAt)
    || first.id.localeCompare(second.id),
})

export const actionItemCommentsSlice = createSlice({
  name: 'actionItemComments',
  initialState: commentsAdapter.getInitialState(),
  reducers: {
    // Replaces one item's comments, so a comment deleted while the page was away goes too.
    actionItemCommentsLoaded(state, action: PayloadAction<{ actionItemId: string, comments: ActionItemComment[] }>) {
      const staleIds: string[] = []
      for (const comment of Object.values(state.entities)) {
        if (comment.actionItemId === action.payload.actionItemId) {
          staleIds.push(comment.id)
        }
      }
      commentsAdapter.removeMany(state, staleIds)
      commentsAdapter.setMany(state, action.payload.comments)
    },
    actionItemCommentUpserted: commentsAdapter.setOne,
    actionItemCommentDeleted(state, action: PayloadAction<string>) {
      commentsAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  actionItemCommentsLoaded,
  actionItemCommentUpserted,
  actionItemCommentDeleted,
} = actionItemCommentsSlice.actions

const { selectAll: selectAllComments } = commentsAdapter.getSelectors(
  (state: RootState) => state.actionItemComments,
)

export const selectActionItemComments = createSelector(
  [ selectAllComments, (_state: RootState, actionItemId: string) => actionItemId ],
  (comments, actionItemId) => comments.filter((comment) => comment.actionItemId === actionItemId),
)
