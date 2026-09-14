// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `LlmType` in api/src/models/llm.rs.
export type LlmType =
  | 'chatgpt-oauth'
  | 'chatgpt-api-token'
  | 'claude-api-token'
  | 'claude-code-oauth'

// The API never returns the secret token. Timestamps are UTC ISO 8601 strings;
// convert to the viewer's timezone only when rendering.
export type Llm = {
  id: string
  name: string
  description: string
  type: LlmType
  priority: number
  isActive: boolean
  expiresAt: string | null
  createdAt: string
  updatedAt: string
}

type ListLlmsResponse = {
  llms: Llm[]
}

export function listLlms() {
  return apiClient
    .get('v1/llms')
    .json<ListLlmsResponse>()
}

type GetLlmResponse = {
  llm: Llm
}

export function getLlm(llmId: string) {
  return apiClient
    .get(`v1/llms/${llmId}`)
    .json<GetLlmResponse>()
}

type CreateLlmRequest = {
  name: string
  description?: string
  type: LlmType
  secretToken: string
  priority?: number
  isActive?: boolean
  // Must carry an offset, e.g. `date.toISOString()`.
  expiresAt?: string | null
}

type CreateLlmResponse = {
  llm: Llm
}

export function createLlm(body: CreateLlmRequest) {
  return apiClient
    .post('v1/llms', { json: body })
    .json<CreateLlmResponse>()
}

// Omitted fields are left unchanged; `expiresAt: null` clears the expiry.
type UpdateLlmRequest = {
  name?: string
  description?: string
  type?: LlmType
  secretToken?: string
  priority?: number
  isActive?: boolean
  expiresAt?: string | null
}

type UpdateLlmResponse = {
  llm: Llm
}

export function updateLlm(llmId: string, body: UpdateLlmRequest) {
  return apiClient
    .patch(`v1/llms/${llmId}`, { json: body })
    .json<UpdateLlmResponse>()
}

export function deleteLlm(llmId: string) {
  return apiClient
    .delete(`v1/llms/${llmId}`)
}
