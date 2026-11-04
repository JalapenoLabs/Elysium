// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  AgentKind,
  AgentModel,
  Effort,
  KindModel,
  ModelChoice,
  ModelSource,
} from '../../../api/routes/modelSettingRoutes'

// Words and rules for the pages that choose models: Settings, Models and a project's Models.
// Pure, and tested beside.

// What the CLI's `--model` may be given: a letter or digit, then up to 99 letters, digits, dots,
// underscores, hyphens, or brackets. Mirrors `agent_models::is_model_name` in the API, which
// refuses anything else, so a model that could read as a flag is flagged before saving.
const MODEL_NAME_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._[\]-]{0,99}$/

export function isModelName(model: string) {
  return MODEL_NAME_PATTERN.test(model)
}

// The product names of the models the API suggests. Proper nouns, so never translated; a
// model without one shows its id.
export const modelProductNames = new Map<string, string>([
  [ 'claude-fable-5-1', 'Claude Fable 5.1' ],
  [ 'claude-opus-5-5', 'Claude Opus 5.5' ],
  [ 'claude-sonnet-5-5', 'Claude Sonnet 5.5' ],
  [ 'claude-haiku-5-5', 'Claude Haiku 5.5' ],
])

export const effortLabelKeys = {
  low: 'efforts.low',
  medium: 'efforts.medium',
  high: 'efforts.high',
  xhigh: 'efforts.xhigh',
  max: 'efforts.max',
} as const satisfies Record<Effort, ParseKeys<'models'>>

export const kindLabelKeys = {
  worker: 'kinds.worker',
  project_manager: 'kinds.project_manager',
  correction: 'kinds.correction',
  segmenting: 'kinds.segmenting',
  triage: 'kinds.triage',
  elysia: 'kinds.elysia',
  summaries: 'kinds.summaries',
} as const satisfies Record<AgentKind, ParseKeys<'models'>>

// Why a step or agent runs on what it does, in the words of the column that says so.
export const sourceLabelKeys = {
  project: 'sources.project',
  agent: 'sources.agent',
  kind: 'sources.kind',
  default: 'sources.default',
} as const satisfies Record<ModelSource, ParseKeys<'models'>>

// The words for each agent outside the step framework, by its name in code. Steps take their
// words from the Runs pages, so both pages name a step alike. The set is open-ended on the
// server, so a name a newer API sends reads as its name in code as words.
export const agentNameKeys = new Map<string, ParseKeys<'models'>>([
  [ 'elysia', 'agents.elysia' ],
  [ 'coding_session', 'agents.coding_session' ],
  [ 'studio_session', 'agents.studio_session' ],
  [ 'turn_summary', 'agents.turn_summary' ],
  [ 'knowledge_ask', 'agents.knowledge_ask' ],
])

// What `kind` runs on across the workspace now: an admin's setting, or its default.
export function currentKindChoice(kind: KindModel): ModelChoice {
  if (kind.setting) {
    return {
      model: kind.setting.model,
      effort: kind.setting.effort,
    }
  }
  return kind.default
}

// What `agent` runs on outside any project now, whatever level it came from. `null` for a
// step that runs no model.
export function currentAgentChoice(agent: AgentModel): ModelChoice | null {
  if (!agent.resolved) {
    return null
  }
  return {
    model: agent.resolved.model,
    effort: agent.resolved.effort,
  }
}

export function isSameChoice(first: ModelChoice | null, second: ModelChoice | null) {
  return first?.model === second?.model && first?.effort === second?.effort
}

// What a page holds that differs from what each row runs on now, in the order listed: what Save
// sends. A change picked back to what is stored is no change, and a change for a row the list no
// longer has is dropped.
export function pendingChanges<Row extends { name: string }>(
  rows: Row[],
  changes: Partial<Record<string, ModelChoice>>,
  currentOf: (row: Row) => ModelChoice | null,
) {
  const pending: { row: Row, choice: ModelChoice }[] = []
  for (const row of rows) {
    const choice = changes[row.name]
    if (!choice || isSameChoice(choice, currentOf(row))) {
      continue
    }
    pending.push({ row, choice })
  }
  return pending
}

// The steps and agents of each kind, by their words, in the order listed, so a kind's row can
// say what it covers. A kind with none yet has no entry.
export function membersByKind(agents: AgentModel[], nameOf: (agent: AgentModel) => string) {
  const members = new Map<AgentKind, string[]>()
  for (const agent of agents) {
    if (!agent.kind) {
      continue
    }
    const names = members.get(agent.kind) ?? []
    names.push(nameOf(agent))
    members.set(agent.kind, names)
  }
  return members
}
