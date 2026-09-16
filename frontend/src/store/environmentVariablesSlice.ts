// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { EnvironmentVariable } from '../api/routes/environmentRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

const environmentVariablesAdapter = createEntityAdapter<EnvironmentVariable>({
  sortComparer: (first, second) => first.key.localeCompare(second.key),
})

export const environmentVariablesSlice = createSlice({
  name: 'environmentVariables',
  initialState: environmentVariablesAdapter.getInitialState(),
  reducers: {
    environmentVariablesLoaded: environmentVariablesAdapter.setAll,
    environmentVariableUpserted: environmentVariablesAdapter.upsertOne,
    environmentVariableDeleted(state, action: PayloadAction<string>) {
      environmentVariablesAdapter.removeOne(state, action.payload)
    },
  },
})

export const {
  environmentVariablesLoaded,
  environmentVariableUpserted,
  environmentVariableDeleted,
} = environmentVariablesSlice.actions

export const {
  selectAll: selectAllEnvironmentVariables,
  selectById: selectEnvironmentVariableById,
} = environmentVariablesAdapter.getSelectors((state: RootState) => state.environmentVariables)
