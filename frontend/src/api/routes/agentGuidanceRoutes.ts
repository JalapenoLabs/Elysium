// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// One version of the guidance every connected agent reads. Mirrors `VersionResponse` in
// api/src/agent_guidance/mod.rs; docs/knowledge.md, Agent guidance.
export type AgentGuidanceVersion = {
  version: number
  // Markdown; empty when the company adds nothing to Elysium's rules.
  content: string
  // The earlier version a restore copied the text of.
  restoredFrom: number | null
  createdBy: string
  createdAt: string
}

// Mirrors `AgentGuidanceResponse`. Every route answers it whole.
export type AgentGuidance = {
  // The version every agent reads now.
  current: AgentGuidanceVersion
  // Newest first, the current one among them.
  versions: AgentGuidanceVersion[]
  // What Elysium tells every agent after the guidance, which no edit removes.
  rules: string
  // The current version as a block for a repository's CLAUDE.md.
  claudeMd: string
  // The longest guidance this deployment takes, so the whole of what an agent reads fits what
  // Claude Code reads of a server's instructions.
  maxCharacters: number
}

// The guidance's key, revalidated by `agentGuidance.changed`.
export const AGENT_GUIDANCE_KEY = 'v1/agent-guidance'

export function getAgentGuidance() {
  return apiClient
    .get(AGENT_GUIDANCE_KEY)
    .json<AgentGuidance>()
}

// A save against a version that is no longer current answers 409 with the code
// `guidance_changed`. Text the current version already holds records nothing.
export function saveAgentGuidance(content: string, baseVersion: number) {
  return apiClient
    .put(AGENT_GUIDANCE_KEY, { json: { content, baseVersion }})
    .json<AgentGuidance>()
}

export function restoreAgentGuidanceVersion(version: number) {
  return apiClient
    .post(`v1/agent-guidance/versions/${version}/restore`)
    .json<AgentGuidance>()
}
