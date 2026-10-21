// Copyright © 2026 Jalapeno Labs

// Misc
import { SESSION_HISTORY_TIMEOUT_MS } from '../../constants'
import { apiClient } from '../index'

// Mirrors the views in api/src/sessions/views.rs. Timestamps are UTC ISO 8601 strings.

// What a session is doing now. Sessions never end on their own: one is idle until it is given a
// prompt, and stays until it is deleted.
export type ActivityState =
  | 'idle'
  // Turns are waiting for a free slot behind other sessions'.
  | 'queued'
  | 'running'

export type SessionActivity = {
  state: ActivityState
  // Turns submitted and not yet started.
  queueDepth: number
  currentTurnId: string | null
  lastActivityAt: string | null
}

export type CodingSession = {
  // The session's number: 1, 2, 3, ... in the order sessions were started.
  id: number
  // Always set for a Coding session; a Studio session has its item's, which may be null.
  projectId: string | null
  title: string
  // The GitHub token the session works with; null for none or a token since deleted.
  githubCredentialId: string | null
  // The action item the session was started from, if any.
  actionItemId: string | null
  // The Studio item the session belongs to; null for a Coding session.
  studioItemId: string | null
  createdAt: string
  // The user who created it: a person, or a machine such as the coding agent.
  createdBy: string
  updatedAt: string
  activity: SessionActivity
}

export type TurnState =
  | 'queued'
  | 'completed'
  | 'failed'
  | 'cancelled'
  | 'interrupted'
  // Every model credential was spent; a later turn resumes once one is back.
  | 'usage-limited'

// The payloads the API renders, tagged by `kind`. Events it does not render arrive
// with `payload: null` and only their `type`.
export type SessionEventPayload =
  | { kind: 'agentMessage', text: string }
  | { kind: 'agentThinking', text: string }
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
  | { kind: 'incident', code: string, message: string, retryable: boolean }

export type SessionEvent = {
  sessionId: number
  // Strictly increasing per session; history and live events merge on it.
  sequence: number
  turnId: string | null
  occurredAt: string | null
  type: string
  payload: SessionEventPayload | null
}

// Coding sessions only: Studio's arrive with their items (`studioRoutes.ts`).
type ListCodingSessionsResponse = {
  sessions: CodingSession[]
}

export function listCodingSessions() {
  return apiClient
    .get('v1/coding-sessions')
    .json<ListCodingSessionsResponse>()
}

// One repository the session clones. Absent `baseBranch` uses the remote's default branch.
export type SessionRepositoryRequest = {
  url: string
  baseBranch?: string
}

type CreateCodingSessionRequest = {
  projectId: string
  title: string
  // Cloned in order, each into its own directory; see sessionRepositories.ts for the rules.
  repositories: SessionRepositoryRequest[]
  // Absent follows the project, null asks for no token, and an id names one.
  githubCredentialId?: string | null
  // Queued as the session's first turn. Required with `actionItemId`.
  prompt?: string
  // The item the session is started from. Its first turn carries the item's context ahead of
  // the prompt, and the session must belong to one of the item's projects, if it has any.
  actionItemId?: string
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

export function renameCodingSession(sessionId: number, title: string) {
  return apiClient
    .patch(`v1/coding-sessions/${sessionId}`, { json: { title }})
    .json<RenameCodingSessionResponse>()
}

export function deleteCodingSession(sessionId: number) {
  return apiClient
    .delete(`v1/coding-sessions/${sessionId}`)
}

// A finished turn's one or two sentence summary, written by a small model after the turn.
export type TurnSummary = {
  sessionId: number
  turnId: string
  summary: string
  createdAt: string
}

export type ListSessionEventsResponse = {
  events: SessionEvent[]
  // True when older events were left out; only the latest few thousand are returned.
  truncated: boolean
  summaries: TurnSummary[]
}

export function listSessionEvents(sessionId: number) {
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

export function startTurn(sessionId: number, prompt: string) {
  return apiClient
    .post(`v1/coding-sessions/${sessionId}/turns`, { json: { prompt }})
    .json<StartTurnResponse>()
}
