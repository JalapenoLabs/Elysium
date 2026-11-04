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
}

const initialState: WorkQueueState = {
  queue: null,
  batchesById: {},
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
  },
})

export const { workQueueLoaded, workBatchLoaded } = workQueueSlice.actions

// The work queue once a page loaded it; null until then.
export function selectWorkQueue(state: RootState) {
  return state.workQueue.queue
}

// Batch `batchId` with its history once its page loaded it; `undefined` until then.
export function selectWorkBatch(state: RootState, batchId: string) {
  return state.workQueue.batchesById[batchId]
}
