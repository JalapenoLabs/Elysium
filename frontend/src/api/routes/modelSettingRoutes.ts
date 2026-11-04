// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// How hard the model thinks: the levels the official CLI's `--effort` takes.
export type Effort = 'low' | 'medium' | 'high' | 'xhigh' | 'max'

// The kinds of agent Elysium runs. Mirrors `AgentKind` in api/src/agent_models.rs.
export type AgentKind =
  | 'worker'
  | 'project_manager'
  | 'correction'
  | 'segmenting'
  | 'triage'
  | 'elysia'
  | 'summaries'

export type ModelChoice = {
  // An id such as `claude-opus-5-5`, or an alias the CLI takes, such as `opus[1m]`.
  model: string
  effort: Effort
}

// What someone chose, and when they last changed it.
export type StoredChoice = ModelChoice & { updatedAt: string }

// Which level a turn's model came from: a project's choice for its kind, the step or agent's
// own setting, its kind's setting, or the kind's default. docs/engine.md, Models.
export type ModelSource = 'project' | 'agent' | 'kind' | 'default'

export type ResolvedChoice = ModelChoice & { source: ModelSource }

// What a kind's turns spent over the list's window, as Claude Code reported it.
export type RecordedUsage = {
  turns: number
  // Millionths of a US dollar: on a subscription, what the same tokens cost on the API.
  costUsdMicros: number
  inputTokens: number
  outputTokens: number
}

// One kind of agent and what it runs on across the workspace. Mirrors `KindModelResponse` in
// api/src/routes/v1/model_settings/mod.rs.
export type KindModel = {
  name: AgentKind
  default: ModelChoice
  // What an admin chose for the kind, or `null` while it runs on its default.
  setting: StoredChoice | null
  // Whether a project may override it for its own agents.
  runsInProjects: boolean
  // `null` where its turns record no cost.
  usage: RecordedUsage | null
}

// One workflow step or agent and what it runs on. Mirrors `AgentModelResponse`.
export type AgentModel = {
  // Its name in code: a step's, as the Runs page names it, or an agent's own, such as `elysia`.
  name: string
  isStep: boolean
  // `null` for a step that runs no model.
  kind: AgentKind | null
  // What an admin chose for it alone, or `null` while it follows its kind.
  setting: StoredChoice | null
  // What its turns run on outside any project, and why; `null` for a step that runs no model.
  resolved: ResolvedChoice | null
}

// A suggested model's price on Anthropic's API, in US cents per million tokens.
export type ModelPrice = {
  model: string
  inputCentsPerMillionTokens: number
  outputCentsPerMillionTokens: number
}

// What every page that sets a model offers to choose from.
export type ModelChoices = {
  // The current Claude models, strongest first. Others the CLI takes may be typed.
  suggestedModels: string[]
  modelPrices: ModelPrice[]
  efforts: Effort[]
}

// The list's key, revalidated by `modelSetting.changed` and `kindModelSetting.changed`.
export const MODEL_SETTINGS_KEY = 'v1/model-settings'

export type ModelSettingsResponse = {
  kinds: KindModel[]
  agents: AgentModel[]
  // Where `usage` starts counting.
  usageSince: string
  choices: ModelChoices
}

export function listModelSettings() {
  return apiClient
    .get(MODEL_SETTINGS_KEY)
    .json<ModelSettingsResponse>()
}

type AgentModelResponse = {
  agent: AgentModel
}

// A choice equal to what it follows without one puts it back on following.
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

type KindModelResponse = {
  kind: KindModel
}

// A choice equal to the kind's default puts it back on its default.
export function setKindModelSetting(kind: AgentKind, choice: ModelChoice) {
  return apiClient
    .put(`${MODEL_SETTINGS_KEY}/kinds/${kind}`, { json: choice })
    .json<KindModelResponse>()
}

export function resetKindModelSetting(kind: AgentKind) {
  return apiClient
    .delete(`${MODEL_SETTINGS_KEY}/kinds/${kind}`)
    .json<KindModelResponse>()
}

// One kind of agent as a project's model settings show it. Mirrors `ProjectKindModelResponse`
// in api/src/routes/v1/projects/list_model_settings.rs.
export type ProjectKindModel = {
  name: AgentKind
  // The project's own choice, or `null` while it follows the workspace.
  setting: StoredChoice | null
  // What the kind runs on without the project's choice: the workspace's setting or the default.
  workspace: ResolvedChoice
  // What its turns for the project spent; `null` where its turns record no cost.
  usage: RecordedUsage | null
}

export type ProjectModelSettingsResponse = {
  kinds: ProjectKindModel[]
  usageSince: string
  choices: ModelChoices
}

// The project's list's key, revalidated by `kindModelSetting.changed`.
export function getProjectModelSettingsKey(projectId: string) {
  return `v1/projects/${projectId}/model-settings`
}

export function listProjectModelSettings(projectId: string) {
  return apiClient
    .get(getProjectModelSettingsKey(projectId))
    .json<ProjectModelSettingsResponse>()
}

type ProjectKindModelResponse = {
  kind: ProjectKindModel
}

// A choice equal to what the workspace runs the kind on removes the project's own.
export function setProjectModelSetting(projectId: string, kind: AgentKind, choice: ModelChoice) {
  return apiClient
    .put(`${getProjectModelSettingsKey(projectId)}/${kind}`, { json: choice })
    .json<ProjectKindModelResponse>()
}

export function resetProjectModelSetting(projectId: string, kind: AgentKind) {
  return apiClient
    .delete(`${getProjectModelSettingsKey(projectId)}/${kind}`)
    .json<ProjectKindModelResponse>()
}
