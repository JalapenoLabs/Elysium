// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type {
  AgentModel,
  Effort,
  ModelSettingsResponse,
} from '../api/routes/modelSettingRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Every step and agent in the order the API lists them: steps first to last, then agents. The
// order is the API's, so the adapter keeps the order each load gives rather than sorting.
const agentModelsAdapter = createEntityAdapter<AgentModel, string>({
  selectId: (agent) => agent.name,
})

type ModelSettingsState = ReturnType<typeof agentModelsAdapter.getInitialState> & {
  suggestedModels: string[]
  efforts: Effort[]
}

const initialState: ModelSettingsState = agentModelsAdapter.getInitialState({
  suggestedModels: [],
  efforts: [],
})

// What each step and agent runs on, which only admins load (`/settings/models`).
export const modelSettingsSlice = createSlice({
  name: 'modelSettings',
  initialState,
  reducers: {
    modelSettingsLoaded(state, action: PayloadAction<ModelSettingsResponse>) {
      agentModelsAdapter.setAll(state, action.payload.agents)
      state.suggestedModels = action.payload.suggestedModels
      state.efforts = action.payload.efforts
    },
    agentModelUpserted(state, action: PayloadAction<AgentModel>) {
      agentModelsAdapter.setOne(state, action.payload)
    },
  },
})

export const { modelSettingsLoaded, agentModelUpserted } = modelSettingsSlice.actions

export const {
  selectAll: selectAgentModels,
} = agentModelsAdapter.getSelectors((state: RootState) => state.modelSettings)

export function selectSuggestedModels(state: RootState) {
  return state.modelSettings.suggestedModels
}

export function selectEfforts(state: RootState) {
  return state.modelSettings.efforts
}
