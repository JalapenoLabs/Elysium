// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { CheckRun, CheckState, Connector } from '../api/routes/connectorRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSlice } from '@reduxjs/toolkit'

const connectorsAdapter = createEntityAdapter<Connector>({
  sortComparer: (first, second) => first.createdAt.localeCompare(second.createdAt),
})

type ConnectorsState = ReturnType<typeof connectorsAdapter.getInitialState> & {
  // Check runs a page is following, by run id, kept current by `connectorCheckRun.updated`.
  checkRuns: Record<string, CheckRun>
}

const initialState: ConnectorsState = {
  ...connectorsAdapter.getInitialState(),
  checkRuns: {},
}

type CheckProgressed = {
  runId: string
  checkId: string | null
  state: CheckState | 'finished'
}

export const connectorsSlice = createSlice({
  name: 'connectors',
  initialState,
  reducers: {
    connectorsLoaded: connectorsAdapter.setAll,
    connectorUpserted: connectorsAdapter.upsertOne,
    connectorDeleted(state, action: PayloadAction<string>) {
      connectorsAdapter.removeOne(state, action.payload)
    },
    checkRunLoaded(state, action: PayloadAction<CheckRun>) {
      state.checkRuns[action.payload.id] = action.payload
    },
    // Moves a check of a followed run along as its event arrives. The event carries states
    // only; the run's messages arrive when it is loaded again after it finishes.
    checkRunProgressed(state, action: PayloadAction<CheckProgressed>) {
      const run = state.checkRuns[action.payload.runId]
      // Every client hears every run; one no page here follows is left alone.
      if (!run) {
        return
      }
      if (action.payload.state === 'finished') {
        run.finishedAt = run.finishedAt ?? new Date().toISOString()
        return
      }
      const check = run.checks.find((progress) => progress.checkId === action.payload.checkId)
      if (check) {
        check.state = action.payload.state
      }
    },
  },
})

export const {
  connectorsLoaded,
  connectorUpserted,
  connectorDeleted,
  checkRunLoaded,
  checkRunProgressed,
} = connectorsSlice.actions

export const {
  selectAll: selectAllConnectors,
  selectById: selectConnectorById,
} = connectorsAdapter.getSelectors((state: RootState) => state.connectors)

export function selectCheckRun(state: RootState, runId: string | null) {
  if (!runId) {
    return null
  }
  return state.connectors.checkRuns[runId] ?? null
}
