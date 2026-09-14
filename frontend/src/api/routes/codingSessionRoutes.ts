// Copyright © 2026 Jalapeno Labs

// Misc
import { SESSION_HISTORY_TIMEOUT_MS } from '../../constants'
import { apiClient } from '../index'

// Mirrors the views in api/src/fleet/views.rs. Timestamps are UTC ISO 8601 strings.

export type ThreadState =
  | 'unknown'
  | 'provisioning'
  | 'idle'
  | 'running'
  | 'awaiting-input'
  | 'watching'
  | 'paused'
  | 'expired'
  | 'destroyed'

export type ThreadStatus = {
  state: ThreadState
  queueDepth: number
  currentTurnId: string | null
  latestSequence: number
  lastActivityAt: string | null
  expiresAt: string | null
}

export type CodingSession = {
  id: string
  satelliteId: string
  threadId: string
  title: string
  createdAt: string
  updatedAt: string
  // The thread as of the API's latest poll; null until the first poll sees it.
  thread: ThreadStatus | null
}

export type TurnState =
  | 'unknown'
  | 'queued'
  | 'running'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted'
  | 'watching'

export type EventAuthor = {
  kind: 'agent' | 'commander' | 'member' | 'subagent' | 'planner' | 'reviewer' | 'suggestions'
  memberId: string | null
  role: string | null
}

// The payloads the API renders, tagged by `kind`. Events it does not render arrive
// with `payload: null` and only their wire `type`.
export type SessionEventPayload =
  | { kind: 'agentMessage', author: EventAuthor | null, text: string }
  | { kind: 'agentThinking', author: EventAuthor | null, text: string }
  | { kind: 'toolStarted', toolCallId: string, toolName: string, input: unknown }
  | {
    kind: 'toolCompleted'
    toolCallId: string
    toolName: string
    ok: boolean
    outputPreview: string | null
    elapsedMilliseconds: number | null
  }
  | { kind: 'turnStarted', prompt: string }
  | { kind: 'turnCompleted', status: TurnState, summary: string, error: string | null }
  | { kind: 'planProposed', body: string }
  | { kind: 'questionAsked', questions: { title: string, detail: string | null, options: string[] }[] }
  | { kind: 'budgetWarning', percentUsed: number }
  | { kind: 'incident', code: string, message: string, retryable: boolean }

export type SessionEvent = {
  sessionId: string
  // Strictly increasing per thread; history and live events merge on it.
  sequence: number
  turnId: string | null
  occurredAt: string | null
  type: string
  memberId: string | null
  payload: SessionEventPayload | null
}

type ListCodingSessionsResponse = {
  sessions: CodingSession[]
}

export function listCodingSessions() {
  return apiClient
    .get('v1/coding-sessions')
    .json<ListCodingSessionsResponse>()
}

type CreateCodingSessionRequest = {
  satelliteId: string
  title: string
  repositoryUrl?: string
  baseBranch?: string
}

type CreateCodingSessionResponse = {
  session: CodingSession
}

export function createCodingSession(body: CreateCodingSessionRequest) {
  return apiClient
    .post('v1/coding-sessions', { json: body })
    .json<CreateCodingSessionResponse>()
}

type RenameCodingSessionResponse = {
  session: CodingSession
}

export function renameCodingSession(sessionId: string, title: string) {
  return apiClient
    .patch(`v1/coding-sessions/${sessionId}`, { json: { title }})
    .json<RenameCodingSessionResponse>()
}

export function deleteCodingSession(sessionId: string) {
  return apiClient
    .delete(`v1/coding-sessions/${sessionId}`)
}

export type ListSessionEventsResponse = {
  events: SessionEvent[]
  // True when older events were left out; only the latest few thousand are returned.
  truncated: boolean
}

export function listSessionEvents(sessionId: string) {
  return apiClient
    .get(`v1/coding-sessions/${sessionId}/events`, { timeout: SESSION_HISTORY_TIMEOUT_MS })
    .json<ListSessionEventsResponse>()
}

type StartTurnResponse = {
  turn: {
    turnId: string
    status: TurnState
    prompt: string
    queuedAt: string | null
  }
}

export function startTurn(sessionId: string, prompt: string) {
  return apiClient
    .post(`v1/coding-sessions/${sessionId}/turns`, { json: { prompt }})
    .json<StartTurnResponse>()
}
