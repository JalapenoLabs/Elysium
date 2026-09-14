// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Llm, LlmType } from '../../../api/routes/llmRoutes'

// Every provider type, in the order the form offers them.
export const LLM_TYPES = [
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
} as const satisfies Record<LlmType, ParseKeys<'llms'>>

export type LlmStatus = 'active' | 'inactive' | 'expired'

export const llmStatusLabelKeys = {
  active: 'status.active',
  inactive: 'status.inactive',
  expired: 'status.expired',
} as const satisfies Record<LlmStatus, ParseKeys<'llms'>>

export const llmStatusChipColors = {
  active: 'success',
  inactive: 'default',
  expired: 'danger',
} as const satisfies Record<LlmStatus, 'success' | 'default' | 'danger'>

// Expiry outranks the active flag: an expired credential cannot be used either way.
export function getLlmStatus(llm: Llm, now: number): LlmStatus {
  if (llm.expiresAt && Date.parse(llm.expiresAt) <= now) {
    return 'expired'
  }

  if (llm.isActive) {
    return 'active'
  }

  return 'inactive'
}
