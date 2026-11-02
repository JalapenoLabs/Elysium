// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `LlmType` in api/src/models/llm.rs. `claude-account` is never created here: it
// comes from signing in (`startClaudeSignIn`), and Elysium keeps its token fresh itself.
export type LlmType =
  | 'chatgpt-oauth'
  | 'chatgpt-api-token'
  | 'claude-api-token'
  | 'claude-code-oauth'
  | 'claude-account'

// One usage window: a percentage used (100 means spent) and when it resets.
export type UsageWindow = {
  utilization: number
  resetsAt: string | null
}

// A Claude account's latest usage, as Anthropic reported it. `readAt` is null until it is read.
export type LlmUsage = {
  fiveHour: UsageWindow | null
  sevenDay: UsageWindow | null
  readAt: string | null
}

// A signed-in Claude account. No token, sealed or not.
export type ClaudeAccount = {
  email: string
  organizationName: string | null
  subscriptionType: string | null
  // False once Anthropic refused the refresh token; only signing in again fixes it.
  isSignedIn: boolean
  accessTokenExpiresAt: string
  lastRefreshedAt: string | null
  lastRefreshError: string | null
  usage: LlmUsage
  usageError: string | null
}

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
  // The user who created it: a person, or a machine such as the coding agent.
  createdBy: string
  updatedAt: string
  // The signed-in account of a `claude-account` credential; null for every other type.
  account: ClaudeAccount | null
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

// A sign-in waiting for its code. `authorizeUrl` opens Anthropic's sign-in page, which shows
// the code to paste back.
export type ClaudeSignIn = {
  id: string
  authorizeUrl: string
  expiresAt: string
}

type StartClaudeSignInResponse = {
  signIn: ClaudeSignIn
}

// Starts the official CLI's sign-in in Elysium's workspace and answers the page to open.
export function startClaudeSignIn() {
  return apiClient
    .post('v1/llms/claude-sign-ins')
    .json<StartClaudeSignInResponse>()
}

type FinishClaudeSignInRequest = {
  code: string
  // Defaults to the account's email.
  name?: string
  priority?: number
}

type FinishClaudeSignInResponse = {
  llm: Llm
}

// Hands the CLI the code from Anthropic's page. An account signed in before keeps its
// credential and takes the new tokens.
export function finishClaudeSignIn(signInId: string, body: FinishClaudeSignInRequest) {
  return apiClient
    .post(`v1/llms/claude-sign-ins/${signInId}`, { json: body })
    .json<FinishClaudeSignInResponse>()
}

export function cancelClaudeSignIn(signInId: string) {
  return apiClient
    .delete(`v1/llms/claude-sign-ins/${signInId}`)
}
