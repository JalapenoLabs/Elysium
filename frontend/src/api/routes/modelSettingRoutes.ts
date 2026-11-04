// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// How hard the model thinks: the levels the official CLI's `--effort` takes.
export type Effort = 'low' | 'medium' | 'high' | 'xhigh' | 'max'

export type ModelChoice = {
  // An id such as `claude-opus-5-5`, or an alias the CLI takes, such as `opus[1m]`.
  model: string
  effort: Effort
}

// One workflow step or agent and what it runs on. Mirrors `AgentModelResponse` in
// api/src/routes/v1/model_settings/mod.rs; docs/engine.md, Models.
export type AgentModel = {
  // Its name in code: a step's, as the Runs page names it, or an agent's own, such as `elysia`.
  name: string
  kind: 'step' | 'agent'
  // What it runs on until an admin chooses otherwise; `null` for a step that runs no model.
  default: ModelChoice | null
  // What an admin chose, or `null` while it runs on its default.
  setting: (ModelChoice & { updatedAt: string }) | null
}

// The list's key, revalidated by `modelSetting.changed`.
export const MODEL_SETTINGS_KEY = 'v1/model-settings'

export type ModelSettingsResponse = {
  agents: AgentModel[]
  // The current Claude models, strongest first. Others the CLI takes may be typed.
  suggestedModels: string[]
  efforts: Effort[]
}

export function listModelSettings() {
  return apiClient
    .get(MODEL_SETTINGS_KEY)
    .json<ModelSettingsResponse>()
}

type AgentModelResponse = {
  agent: AgentModel
}

// A choice equal to the default puts the agent back on it.
export function setModelSetting(name: string, choice: ModelChoice) {
  return apiClient
    .put(`${MODEL_SETTINGS_KEY}/${name}`, { json: choice })
    .json<AgentModelResponse>()
}

export function resetModelSetting(name: string) {
  return apiClient
    .delete(`${MODEL_SETTINGS_KEY}/${name}`)
    .json<AgentModelResponse>()
}
