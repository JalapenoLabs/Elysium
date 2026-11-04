// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { WorkBatchWithHistory, WorkQueue } from '../api/routes/workQueueRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type WorkQueueState = {
  // The pool and its queue, replaced whole by every load: its order is the server's to derive.
  queue: WorkQueue | null
  // The batches whose own page was opened, each with its history, replaced whole by every load.
  batchesById: Record<string, WorkBatchWithHistory>
  // The worker logs opened, by batch. Apart from the batch, whose every load replaces it whole.
  workerLogsById: Record<string, string>
}

const initialState: WorkQueueState = {
  queue: null,
  batchesById: {},
  workerLogsById: {},
}

export const workQueueSlice = createSlice({
  name: 'workQueue',
  initialState,
  reducers: {
    workQueueLoaded(state, action: PayloadAction<WorkQueue>) {
      state.queue = action.payload
    },
    workBatchLoaded(state, action: PayloadAction<WorkBatchWithHistory>) {
      state.batchesById[action.payload.batch.id] = action.payload
    },
    workerLogLoaded(state, action: PayloadAction<{ batchId: string, log: string }>) {
      state.workerLogsById[action.payload.batchId] = action.payload.log
    },
  },
})

export const { workQueueLoaded, workBatchLoaded, workerLogLoaded } = workQueueSlice.actions

// The work queue once a page loaded it; null until then.
export function selectWorkQueue(state: RootState) {
  return state.workQueue.queue
}

// Batch `batchId` with its history once its page loaded it; `undefined` until then.
export function selectWorkBatch(state: RootState, batchId: string) {
  return state.workQueue.batchesById[batchId]
}

// Batch `batchId`'s worker log once its page opened it; `undefined` until then.
export function selectWorkerLog(state: RootState, batchId: string) {
  return state.workQueue.workerLogsById[batchId]
}
