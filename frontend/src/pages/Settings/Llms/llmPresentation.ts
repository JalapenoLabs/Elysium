// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Llm, LlmType, UsageWindow } from '../../../api/routes/llmRoutes'

// Every provider type, in the order the Add LLM menu offers them. A signed-in Claude account
// comes first: Elysium keeps it fresh and can show its usage.
export const LLM_TYPES = [
  'claude-account',
  'claude-api-token',
  'claude-code-oauth',
  'chatgpt-api-token',
  'chatgpt-oauth',
] as const satisfies readonly LlmType[]

export const llmTypeLabelKeys = {
  'chatgpt-oauth': 'types.chatgpt-oauth',
  'chatgpt-api-token': 'types.chatgpt-api-token',
  'claude-api-token': 'types.claude-api-token',
  'claude-code-oauth': 'types.claude-code-oauth',
  'claude-account': 'types.claude-account',
} as const satisfies Record<LlmType, ParseKeys<'llms'>>

// Whether a person types the type's token in. A Claude account's token comes from signing
// in, and the keeper owns its lifetime, so its form has neither a token nor an expiry.
export const isTokenTypedByType = {
  'chatgpt-oauth': true,
  'chatgpt-api-token': true,
  'claude-api-token': true,
  'claude-code-oauth': true,
  'claude-account': false,
} as const satisfies Record<LlmType, boolean>

export type LlmStatus = 'active' | 'inactive' | 'expired' | 'signInExpired'

export const llmStatusLabelKeys = {
  active: 'status.active',
  inactive: 'status.inactive',
  expired: 'status.expired',
  signInExpired: 'status.signInExpired',
} as const satisfies Record<LlmStatus, ParseKeys<'llms'>>

export const llmStatusChipColors = {
  active: 'success',
  inactive: 'default',
  expired: 'danger',
  signInExpired: 'danger',
} as const satisfies Record<LlmStatus, 'success' | 'default' | 'danger'>

// Expiry outranks the active flag: an expired credential cannot be used either way. A Claude
// account whose sign-in Anthropic refused cannot be used either, until it is signed in again.
export function getLlmStatus(llm: Llm, now: number): LlmStatus {
  if (llm.expiresAt && Date.parse(llm.expiresAt) <= now) {
    return 'expired'
  }

  if (llm.account && !llm.account.isSignedIn) {
    return 'signInExpired'
  }

  if (llm.isActive) {
    return 'active'
  }

  return 'inactive'
}

// The types agent turns can run on: the Claude Code CLI takes a subscription token or an
// API key. Mirrors `secret_for` in api/src/runner/credentials.rs.
const AGENT_LLM_TYPES: ReadonlySet<LlmType> = new Set([
  'claude-api-token',
  'claude-code-oauth',
  'claude-account',
])

// Whether an agent turn has a credential to run on, decided as the runner decides it. Without
// one the turn still starts, on whatever login the workspace holds, and normally fails.
export function hasAgentCredential(llms: Llm[], now: number) {
  return llms.some((llm) => AGENT_LLM_TYPES.has(llm.type) && getLlmStatus(llm, now) === 'active')
}

// The usage at which turns stop starting on an account. Mirrors `SKIP_AT_UTILIZATION` in
// api/src/claude_accounts/usage.rs.
export const USAGE_SKIP_AT = 99

export type UsageLevel = 'low' | 'high' | 'spent'

// How full a window looks: spent once turns skip the account, high from three quarters.
export function getUsageLevel(window: UsageWindow): UsageLevel {
  if (window.utilization >= USAGE_SKIP_AT) {
    return 'spent'
  }
  if (window.utilization >= 75) {
    return 'high'
  }
  return 'low'
}

export const usageLevelColors = {
  low: 'success',
  high: 'warning',
  spent: 'danger',
} as const satisfies Record<UsageLevel, 'success' | 'warning' | 'danger'>
