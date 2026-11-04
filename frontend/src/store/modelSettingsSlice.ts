// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type {
  AgentKind,
  AgentModel,
  KindModel,
  ModelChoices,
  ModelSettingsResponse,
} from '../api/routes/modelSettingRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

// Every kind, then every step and agent, each in the order the API lists them: kinds as the
// page offers them, steps first to last, then agents. The order is the API's, so the adapters
// keep the order each load gives rather than sorting.
const kindModelsAdapter = createEntityAdapter<KindModel, AgentKind>({
  selectId: (kind) => kind.name,
})

const agentModelsAdapter = createEntityAdapter<AgentModel, string>({
  selectId: (agent) => agent.name,
})

type ModelSettingsState = {
  kinds: ReturnType<typeof kindModelsAdapter.getInitialState>
  agents: ReturnType<typeof agentModelsAdapter.getInitialState>
  choices: ModelChoices
  // Where each kind's usage starts counting, or null before the first load.
  usageSince: string | null
}

const initialState: ModelSettingsState = {
  kinds: kindModelsAdapter.getInitialState(),
  agents: agentModelsAdapter.getInitialState(),
  choices: {
    suggestedModels: [],
    modelPrices: [],
    efforts: [],
  },
  usageSince: null,
}

// What each kind, step, and agent runs on across the workspace, which only admins load
// (`/settings/models`).
export const modelSettingsSlice = createSlice({
  name: 'modelSettings',
  initialState,
  reducers: {
    modelSettingsLoaded(state, action: PayloadAction<ModelSettingsResponse>) {
      kindModelsAdapter.setAll(state.kinds, action.payload.kinds)
      agentModelsAdapter.setAll(state.agents, action.payload.agents)
      state.choices = action.payload.choices
      state.usageSince = action.payload.usageSince
    },
    agentModelUpserted(state, action: PayloadAction<AgentModel>) {
      agentModelsAdapter.setOne(state.agents, action.payload)
    },
    kindModelUpserted(state, action: PayloadAction<KindModel>) {
      kindModelsAdapter.setOne(state.kinds, action.payload)
    },
  },
})

export const { modelSettingsLoaded, agentModelUpserted, kindModelUpserted } = modelSettingsSlice.actions

export const {
  selectAll: selectKindModels,
} = kindModelsAdapter.getSelectors((state: RootState) => state.modelSettings.kinds)

export const {
  selectAll: selectAgentModels,
} = agentModelsAdapter.getSelectors((state: RootState) => state.modelSettings.agents)

export function selectModelChoices(state: RootState) {
  return state.modelSettings.choices
}

export function selectUsageSince(state: RootState) {
  return state.modelSettings.usageSince
}
