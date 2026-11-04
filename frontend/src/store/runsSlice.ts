// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { RunDetail, RunHistoryEntry, RunStep, WorkflowRun } from '../api/routes/runRoutes'
import type { RootState } from './index'

// Core
import { createEntityAdapter, createSelector, createSlice } from '@reduxjs/toolkit'

// Newest first. Ids are UUIDv7, so they order runs the way they were started.
const runsAdapter = createEntityAdapter<WorkflowRun>({
  sortComparer: (first, second) => second.id.localeCompare(first.id),
})

const stepsAdapter = createEntityAdapter<RunStep>()

type RunsState = {
  runs: ReturnType<typeof runsAdapter.getInitialState>
  steps: ReturnType<typeof stepsAdapter.getInitialState>
  // Each run's step ids in the order they run, for the runs a page has shown.
  stepIdsByRun: Record<string, string[]>
  historyByRun: Record<string, RunHistoryEntry[]>
  // The logs a page has opened, by step. A step whose log is here has its log read again
  // whenever it writes to it.
  logsByStep: Record<string, string>
  // Whether the Runs list has been read, so the event stream knows a new run is worth loading.
  isListLoaded: boolean
}

const initialState: RunsState = {
  runs: runsAdapter.getInitialState(),
  steps: stepsAdapter.getInitialState(),
  stepIdsByRun: {},
  historyByRun: {},
  logsByStep: {},
  isListLoaded: false,
}

export const runsSlice = createSlice({
  name: 'runs',
  initialState,
  reducers: {
    // A page of the list. Pages merge, since runs are never deleted.
    runsPageLoaded(state, action: PayloadAction<WorkflowRun[]>) {
      runsAdapter.upsertMany(state.runs, action.payload)
      state.isListLoaded = true
    },
    // One run as its page shows it, replacing what was known of its steps and history.
    runLoaded(state, action: PayloadAction<RunDetail>) {
      const { run, steps, history } = action.payload
      runsAdapter.setOne(state.runs, run)
      stepsAdapter.removeMany(state.steps, state.stepIdsByRun[run.id] ?? [])
      stepsAdapter.setMany(state.steps, steps)
      state.stepIdsByRun[run.id] = steps.map((step) => step.id)
      state.historyByRun[run.id] = history
    },
    stepLogLoaded(state, action: PayloadAction<{ stepId: string, log: string }>) {
      state.logsByStep[action.payload.stepId] = action.payload.log
    },
  },
})

export const { runsPageLoaded, runLoaded, stepLogLoaded } = runsSlice.actions

const {
  selectAll: selectAllRuns,
  selectById: selectRunEntity,
} = runsAdapter.getSelectors((state: RootState) => state.runs.runs)

const { selectEntities: selectStepEntities } = stepsAdapter.getSelectors(
  (state: RootState) => state.runs.steps,
)

export const selectRuns = selectAllRuns

export function selectRunById(state: RootState, runId: string) {
  return selectRunEntity(state, runId)
}

const NO_STEPS: RunStep[] = []
const NO_HISTORY: RunHistoryEntry[] = []

// Run `runId`'s steps in the order they run, or none until its page has loaded it.
export const selectStepsOfRun = createSelector(
  [
    selectStepEntities,
    (state: RootState, runId: string) => state.runs.stepIdsByRun[runId],
  ],
  (stepsById, stepIds) => {
    if (!stepIds) {
      return NO_STEPS
    }
    const steps: RunStep[] = []
    for (const stepId of stepIds) {
      const step = stepsById[stepId]
      if (step) {
        steps.push(step)
      }
    }
    return steps
  },
)

export function selectRunHistory(state: RootState, runId: string) {
  return state.runs.historyByRun[runId] ?? NO_HISTORY
}

// A step's log once a page opened it; `undefined` until then.
export function selectStepLog(state: RootState, stepId: string) {
  return state.runs.logsByStep[stepId]
}
