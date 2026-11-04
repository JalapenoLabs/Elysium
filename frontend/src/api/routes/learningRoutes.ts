// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// The fixed list a learning is filed under. Mirrors `LearningCategory` in
// api/src/models/learning.rs; docs/knowledge.md says what each one holds.
export const LEARNING_CATEGORIES = [
  'decision',
  'convention',
  'procedure',
  'pitfall',
  'reference',
  'environment',
  'people',
  'project',
  'incident',
] as const
export type LearningCategory = typeof LEARNING_CATEGORIES[number]

// The agent that acted for someone: an MCP client, by its id (`mcp:<client id>`) and the
// name it registered, which is a label only.
export type LearningAgent = {
  id: string
  name: string | null
}

// Mirrors `LearningResponse` in api/src/routes/v1/learnings/mod.rs.
export type Learning = {
  id: string
  title: string
  body: string
  category: LearningCategory
  evidence: string
  // The agent acting for its creator when it was first saved; null when they saved it.
  agent: LearningAgent | null
  // Who first saved it. Who changed it since is in its history.
  createdBy: string
  createdAt: string
  updatedAt: string
  // Set while it is deleted; it can be restored.
  deletedAt: string | null
}

export const LEARNING_HISTORY_KINDS = [ 'created', 'revised', 'deleted', 'restored' ] as const
export type LearningHistoryKind = typeof LEARNING_HISTORY_KINDS[number]

// One change to a learning. `kind` is text, so a kind a later API writes still loads; `data`
// is shaped by it (docs/knowledge.md, History and undo). Mirrors `HistoryEntryResponse`.
export type LearningHistoryEntry = {
  id: string
  learningId: string
  kind: string
  createdBy: string
  agent: LearningAgent | null
  data: Record<string, unknown>
  createdAt: string
}

export type LearningFeedEntry = {
  entry: LearningHistoryEntry
  learning: Learning
}

export type LearningContent = {
  title: string
  body: string
  category: LearningCategory
  evidence: string
}

type LearningsResponse = {
  learnings: Learning[]
}

type ListLearningsOptions = {
  deleted?: boolean
}

export function listLearnings(options: ListLearningsOptions = {}) {
  const searchParams = options.deleted
    ? { deleted: 'true' }
    : undefined
  return apiClient
    .get('v1/learnings', { searchParams })
    .json<LearningsResponse>()
}

type LearningResponse = {
  learning: Learning
}

export function getLearning(learningId: string) {
  return apiClient
    .get(`v1/learnings/${learningId}`)
    .json<LearningResponse>()
}

// What saving did: a title no learning had makes a new one, a title one had revises it, and
// the same words again change nothing.
export type SaveOutcome = 'created' | 'revised' | 'unchanged'

type SaveLearningResponse = {
  learning: Learning
  outcome: SaveOutcome
}

export function saveLearning(body: LearningContent) {
  return apiClient
    .post('v1/learnings', { json: body })
    .json<SaveLearningResponse>()
}

// Omitted fields keep their value.
export function correctLearning(learningId: string, body: Partial<LearningContent>) {
  return apiClient
    .patch(`v1/learnings/${learningId}`, { json: body })
    .json<LearningResponse>()
}

export function deleteLearning(learningId: string) {
  return apiClient.delete(`v1/learnings/${learningId}`)
}

export function restoreLearning(learningId: string) {
  return apiClient
    .post(`v1/learnings/${learningId}/restore`)
    .json<LearningResponse>()
}

type LearningHistoryResponse = {
  history: LearningHistoryEntry[]
}

export function learningHistoryKey(learningId: string) {
  return `v1/learnings/${learningId}/history`
}

export function listLearningHistory(learningId: string) {
  return apiClient
    .get(learningHistoryKey(learningId))
    .json<LearningHistoryResponse>()
}

// Reverses one change, recorded as a change of its own; `409` when it has nothing left to
// reverse.
export function undoLearningChange(learningId: string, entryId: string) {
  return apiClient
    .post(`v1/learnings/${learningId}/history/${entryId}/undo`)
    .json<LearningResponse>()
}

// The admins' feed's first page; the event stream revalidates it.
export const LEARNING_FEED_KEY = 'v1/learnings/feed'

type LearningFeedResponse = {
  entries: LearningFeedEntry[]
  // Pass as `before` for the next page; null at the end.
  nextBefore: string | null
}

export function getLearningFeed(before: string | null) {
  const searchParams = before
    ? { before }
    : undefined
  return apiClient
    .get(LEARNING_FEED_KEY, { searchParams })
    .json<LearningFeedResponse>()
}
