// Copyright © 2026 Jalapeno Labs

import type { KnowledgeAsk } from '../api/routes/knowledgeRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// The person's own questions, newest first, as the Ask tab lists them. Ids are UUIDv7, so ties
// in the same millisecond still read in the order they were asked.
const knowledgeAsksAdapter = createEntityAdapter<KnowledgeAsk>({
  sortComparer: (first, second) => second.createdAt.localeCompare(first.createdAt)
    || second.id.localeCompare(first.id),
})

export const knowledgeAsksSlice = createSlice({
  name: 'knowledgeAsks',
  initialState: knowledgeAsksAdapter.getInitialState(),
  reducers: {
    knowledgeAsksLoaded: knowledgeAsksAdapter.setAll,
    knowledgeAskUpserted: knowledgeAsksAdapter.setOne,
  },
})

export const { knowledgeAsksLoaded, knowledgeAskUpserted } = knowledgeAsksSlice.actions

export const {
  selectAll: selectAllKnowledgeAsks,
  selectById: selectKnowledgeAskById,
} = knowledgeAsksAdapter.getSelectors((state: RootState) => state.knowledgeAsks)
