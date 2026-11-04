// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `Status` in api/src/project_managers/status.rs.
export const PROJECT_MANAGER_STATUSES = [ 'running', 'held', 'errored', 'waiting', 'idle' ] as const
export type ProjectManagerStatus = typeof PROJECT_MANAGER_STATUSES[number]

// Mirrors `TurnStatus` in api/src/models/project_manager.rs.
export const PROJECT_MANAGER_TURN_STATUSES = [ 'running', 'succeeded', 'failed' ] as const
export type ProjectManagerTurnStatus = typeof PROJECT_MANAGER_TURN_STATUSES[number]

// Mirrors `WakeKind` in api/src/models/project_manager.rs.
export const PROJECT_MANAGER_WAKE_KINDS = [ 'record', 'batch', 'message' ] as const
export type ProjectManagerWakeKind = typeof PROJECT_MANAGER_WAKE_KINDS[number]

// Mirrors `Priority` in api/src/models/work_batch.rs.
export const WORK_BATCH_PRIORITIES = [ 'asap', 'normal', 'scheduled' ] as const
export type WorkBatchPriority = typeof WORK_BATCH_PRIORITIES[number]

// Mirrors `BatchState` in api/src/models/work_batch.rs.
export const WORK_BATCH_STATES = [ 'queued', 'running', 'succeeded', 'failed', 'cancelled' ] as const
export type WorkBatchState = typeof WORK_BATCH_STATES[number]

// One thing that woke a turn: a message with its text, a record, or a batch, by id.
export type ProjectManagerWake = {
  id: string
  kind: ProjectManagerWakeKind
  timelineRecordId: string | null
  workBatchId: string | null
  body: string | null
  // Who caused it: the person or Elysia who wrote, or whoever attributed the record.
  createdBy: string
  createdAt: string
}

// One turn, collapsed to its summary, with what woke it and what it answered. Its log is read
// alone (`getProjectManagerTurn`).
export type ProjectManagerTurn = {
  id: string
  status: ProjectManagerTurnStatus
  summary: string | null
  reply: string | null
  error: string | null
  timelineRecordId: string | null
  startedAt: string
  finishedAt: string | null
  wakes: ProjectManagerWake[]
}

export type WorkBatchRepository = {
  url: string
  baseBranch?: string
}

// One batch of work a Project Manager submitted for a worker.
export type WorkBatch = {
  id: string
  title: string
  goal: string
  repositories: WorkBatchRepository[]
  priority: WorkBatchPriority
  // When a scheduled batch is due.
  scheduledFor: string | null
  state: WorkBatchState
  // What its worker reported, or why it was cancelled.
  result: string | null
  // The turn that submitted it.
  turnId: string | null
  sourceRecordIds: string[]
  actionItemIds: string[]
  timelineRecordId: string | null
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
}

// A project's Project Manager, as its page shows it.
export type ProjectManager = {
  projectId: string
  status: ProjectManagerStatus
  // How many wakes wait for its next turn.
  waiting: number
  // While a usage limit holds it, when it lifts.
  usagePausedUntil: string | null
  // Its latest turns, newest first.
  turns: ProjectManagerTurn[]
  // Its latest batches, newest first.
  batches: WorkBatch[]
}

export function getProjectManagerKey(projectId: string) {
  return `v1/projects/${projectId}/project-manager`
}

export function getProjectManagerTurnKey(projectId: string, turnId: string) {
  return `${getProjectManagerKey(projectId)}/turns/${turnId}`
}

type ProjectManagerResponse = {
  projectManager: ProjectManager
}

export function getProjectManager(projectId: string) {
  return apiClient
    .get(getProjectManagerKey(projectId))
    .json<ProjectManagerResponse>()
}

type ProjectManagerTurnResponse = {
  turn: ProjectManagerTurn
  // What it said and which tools it called, one line each.
  log: string
}

export function getProjectManagerTurn(projectId: string, turnId: string) {
  return apiClient
    .get(getProjectManagerTurnKey(projectId, turnId))
    .json<ProjectManagerTurnResponse>()
}

export function sendProjectManagerMessage(projectId: string, body: string) {
  return apiClient
    .post(`${getProjectManagerKey(projectId)}/messages`, { json: { body }})
    .json<ProjectManagerResponse>()
}
