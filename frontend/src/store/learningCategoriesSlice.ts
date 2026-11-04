// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { LearningCategory } from '../api/routes/learningCategoryRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// In the order the API offers them: the built-in categories first, then the workspace's own,
// each in its order. A key never changes, so it is the id.
const learningCategoriesAdapter = createEntityAdapter<LearningCategory, string>({
  selectId: (category) => category.key,
  sortComparer: (first, second) => Number(second.builtIn) - Number(first.builtIn)
    || first.position - second.position
    || first.key.localeCompare(second.key),
})

export const learningCategoriesSlice = createSlice({
  name: 'learningCategories',
  initialState: learningCategoriesAdapter.getInitialState(),
  reducers: {
    learningCategoriesLoaded(state, action: PayloadAction<LearningCategory[]>) {
      learningCategoriesAdapter.setAll(state, action.payload)
    },
    learningCategoryUpserted(state, action: PayloadAction<LearningCategory>) {
      learningCategoriesAdapter.setOne(state, action.payload)
    },
    learningCategoryRemoved(state, action: PayloadAction<string>) {
      learningCategoriesAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  learningCategoriesLoaded,
  learningCategoryUpserted,
  learningCategoryRemoved,
} = learningCategoriesSlice.actions

export const {
  selectAll: selectLearningCategories,
  selectById: selectLearningCategoryByKey,
} = learningCategoriesAdapter.getSelectors((state: RootState) => state.learningCategories)
