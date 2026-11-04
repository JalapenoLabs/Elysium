// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { AgentModel, Effort, ModelChoice } from '../../../api/routes/modelSettingRoutes'

// Words and rules for the model settings page. Pure, and tested beside.

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
  step: 'kinds.step',
  agent: 'kinds.agent',
} as const satisfies Record<AgentModel['kind'], ParseKeys<'models'>>

// The words for each agent outside the step framework, by its name in code. Steps take their
// words from the Runs pages, so both pages name a step alike. The set is open-ended on the
// server, so a name a newer API sends reads as its name in code as words.
export const agentNameKeys = new Map<string, ParseKeys<'models'>>([
  [ 'elysia', 'agents.elysia' ],
  [ 'coding_session', 'agents.coding_session' ],
  [ 'studio_session', 'agents.studio_session' ],
  [ 'turn_summary', 'agents.turn_summary' ],
  [ 'segment_meeting', 'agents.segment_meeting' ],
])

// What `agent` runs on now: its setting, or its default. `null` for a step that runs no model.
export function currentChoice(agent: AgentModel): ModelChoice | null {
  if (agent.setting) {
    return {
      model: agent.setting.model,
      effort: agent.setting.effort,
    }
  }
  return agent.default
}

export function isSameChoice(first: ModelChoice | null, second: ModelChoice | null) {
  return first?.model === second?.model && first?.effort === second?.effort
}

// What the page holds that differs from what each agent runs on now, in the order listed:
// what Save sends. A change typed back to what is stored is no change.
export function pendingChanges(agents: AgentModel[], changes: Record<string, ModelChoice>) {
  const pending: { agent: AgentModel, choice: ModelChoice }[] = []
  for (const agent of agents) {
    const choice = changes[agent.name]
    if (!choice || isSameChoice(choice, currentChoice(agent))) {
      continue
    }
    pending.push({ agent, choice })
  }
  return pending
}
