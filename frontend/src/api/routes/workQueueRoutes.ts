// Copyright © 2026 Jalapeno Labs

import type { WorkBatch, WorkBatchPriority, WorkBatchState } from './projectManagerRoutes'

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
}

// Mirrors `EventKind` in api/src/models/work_batch.rs.
export const WORK_BATCH_EVENT_KINDS = [
  'submitted',
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

export type WorkBatchWithHistory = {
  batch: WorkBatch
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

export function cancelWorkBatch(batchId: string, reason: string | null) {
  return apiClient
    .post(`${getWorkBatchKey(batchId)}/cancel`, { json: { reason }})
    .json<WorkBatchWithHistory>()
}
