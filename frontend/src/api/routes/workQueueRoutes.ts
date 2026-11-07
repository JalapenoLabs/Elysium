// Copyright © 2026 Jalapeno Labs

import type { ActionItemState } from './actionItemRoutes'
import type {
  WorkBatch,
  WorkBatchPriority,
  WorkBatchRepository,
  WorkBatchState,
} from './projectManagerRoutes'

// Misc
import { apiClient } from '../index'

export const WORK_QUEUE_KEY = 'v1/work-queue'

// The worker pool: how many workers run at once and how many slots are in use.
export type WorkerPool = {
  // Null until the engine has looked, when no admin chose a size.
  size: number | null
  // The size an admin chose; null follows the default.
  configuredSize: number | null
  // One per CPU core the engine sees minus two, at least one; null until the engine has looked.
  defaultSize: number | null
  cpuCount: number | null
  inUse: number
  // While every Claude account is at its usage limit, when the first resets.
  usageHeldUntil: string | null
}

// A batch as the queue lists it, without its goal, which its own page reads.
export type WorkQueueEntry = {
  id: string
  projectId: string
  title: string
  priority: WorkBatchPriority
  scheduledFor: string | null
  state: WorkBatchState
  // While someone asked for it to run next.
  pinnedAt: string | null
  // When the pool gave it a slot.
  assignedAt: string | null
  createdAt: string
  startedAt: string | null
}

// Mirrors `QueueResponse` in api/src/routes/v1/work_queue.rs.
export type WorkQueue = {
  pool: WorkerPool
  // The batches holding slots, in the order they were handed out.
  inSlots: WorkQueueEntry[]
  // Every batch the pool may hand a slot now, first first.
  next: WorkQueueEntry[]
  // Scheduled batches not yet due, soonest first.
  scheduled: WorkQueueEntry[]
  paused: WorkQueueEntry[]
  // Batches waiting for a person's approval, oldest first; the pool hands them nothing.
  proposed: WorkQueueEntry[]
}

// Mirrors `EventKind` in api/src/models/work_batch.rs.
export const WORK_BATCH_EVENT_KINDS = [
  'submitted',
  'held_for_approval',
  'edited',
  'approved',
  'rejected',
  'pinned',
  'unpinned',
  'reprioritized',
  'paused',
  'resumed',
  'cancelled',
  'assigned',
  'unassigned',
  'started',
  'waiting',
  'answered',
  'finished',
] as const
export type WorkBatchEventKind = typeof WORK_BATCH_EVENT_KINDS[number]

// One entry of a batch's history. `kind` is a string, since a later build may write kinds this
// one does not know; `data` is shaped by the kind (`docs/engine.md`, Work batches).
export type WorkBatchHistoryEntry = {
  id: string
  kind: string
  data: Record<string, unknown>
  createdBy: string
  createdAt: string
}

type WorkQueueResponse = {
  workQueue: WorkQueue
}

export function getWorkQueue() {
  return apiClient
    .get(WORK_QUEUE_KEY)
    .json<WorkQueueResponse>()
}

export function getWorkBatchKey(batchId: string) {
  return `v1/work-batches/${batchId}`
}

// A timeline record that called for a batch, opened through the knowledge base, where any member
// reads what was ingested.
export type WorkBatchCause = {
  id: string
  sourceKind: string
  title: string
  occurredAt: string
  // Null until the knowledge base has indexed it.
  documentId: string | null
}

// An action item a batch works on.
export type WorkBatchItem = {
  id: string
  title: string
  state: ActionItemState
}

// Mirrors `WhyResponse` in api/src/routes/v1/work_batches/mod.rs: what called for a batch.
export type WorkBatchWhy = {
  records: WorkBatchCause[]
  actionItems: WorkBatchItem[]
}

export type WorkBatchWithHistory = {
  batch: WorkBatch
  why: WorkBatchWhy
  history: WorkBatchHistoryEntry[]
}

export function getWorkBatch(batchId: string) {
  return apiClient
    .get(getWorkBatchKey(batchId))
    .json<WorkBatchWithHistory>()
}

export function getWorkerLogKey(batchId: string) {
  return `${getWorkBatchKey(batchId)}/worker-log`
}

type WorkerLogResponse = {
  // What its worker said and which tools it called, one line each, the newest 64 KiB. Empty
  // before its worker starts.
  log: string
}

export function getWorkerLog(batchId: string) {
  return apiClient
    .get(getWorkerLogKey(batchId))
    .json<WorkerLogResponse>()
}

// A pull request a batch's worker recorded, mirroring `BatchPullRequest` in
// api/src/models/work_batch/pull_request.rs.
export type BatchPullRequest = {
  id: string
  workBatchId: string
  // `owner/name`, as GitHub spells it.
  repository: string
  number: number
  url: string
  title: string
  state: 'open' | 'merged' | 'closed'
  mergedAt: string | null
  // The login GitHub says merged it.
  mergedBy: string | null
  mergedByElysium: boolean
  mergeMethod: 'squash' | 'merge' | 'rebase' | null
  // Why the last look left it open.
  heldBecause: string | null
  lookedAt: string | null
  timelineRecordId: string | null
  createdBy: string
  createdAt: string
}

export function getWorkBatchPullRequestsKey(batchId: string) {
  return `${getWorkBatchKey(batchId)}/pull-requests`
}

type WorkBatchPullRequestsResponse = {
  pullRequests: BatchPullRequest[]
}

export function getWorkBatchPullRequests(batchId: string) {
  return apiClient
    .get(getWorkBatchPullRequestsKey(batchId))
    .json<WorkBatchPullRequestsResponse>()
}

// Each field left out stays as it is.
export type WorkBatchChanges = {
  priority?: WorkBatchPriority
  // With its offset, beside `priority: 'scheduled'`.
  scheduledFor?: string
  paused?: boolean
  runNext?: boolean
}

export function updateWorkBatch(batchId: string, changes: WorkBatchChanges) {
  return apiClient
    .patch(getWorkBatchKey(batchId), { json: changes })
    .json<WorkBatchWithHistory>()
}

// What a person changes as they approve a proposed batch; each field left out stays as its
// Project Manager proposed it.
export type WorkBatchEdits = {
  title?: string
  goal?: string
  // Replaces every repository it named.
  repositories?: WorkBatchRepository[]
  priority?: WorkBatchPriority
  // With its offset, beside `priority: 'scheduled'`.
  scheduledFor?: string
}

export function approveWorkBatch(batchId: string, edits: WorkBatchEdits) {
  return apiClient
    .post(`${getWorkBatchKey(batchId)}/approve`, { json: edits })
    .json<WorkBatchWithHistory>()
}

// Declines a proposed batch; its Project Manager is told the reason.
export function rejectWorkBatch(batchId: string, reason: string) {
  return apiClient
    .post(`${getWorkBatchKey(batchId)}/reject`, { json: { reason }})
    .json<WorkBatchWithHistory>()
}

export function cancelWorkBatch(batchId: string, reason: string | null) {
  return apiClient
    .post(`${getWorkBatchKey(batchId)}/cancel`, { json: { reason }})
    .json<WorkBatchWithHistory>()
}
