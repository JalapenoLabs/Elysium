// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `WorkflowRunState` in api/src/models/workflow_run.rs.
export const RUN_STATES = [ 'queued', 'running', 'held', 'failed', 'succeeded' ] as const
export type RunState = typeof RUN_STATES[number]

// Mirrors `StepJobState` in api/src/models/step_job.rs.
export const STEP_STATES = [ 'pending', 'queued', 'running', 'held', 'failed', 'skipped', 'succeeded' ] as const
export type StepState = typeof STEP_STATES[number]

// One workflow on one source. Its input stays on the server.
export type WorkflowRun = {
  id: string
  // The workflow's name in code, such as `meeting`.
  workflow: string
  sourceKind: string
  sourceId: string
  state: RunState
  // Why it stopped, while it is failed.
  error: string | null
  // Who or what started it: a person, or a machine such as Elysia.
  createdBy: string
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
}

// One step of a run. Its input, output, and log stay on the server; the log is read on its own.
export type RunStep = {
  id: string
  runId: string
  // The step's name in code, such as `fetch_transcript`.
  step: string
  state: StepState
  // The one line it collapses to, which the step wrote.
  summary: string | null
  // Why its last attempt failed.
  error: string | null
  attempts: number
  // While it backs off, when it is tried again.
  availableAt: string | null
  // When its last attempt started and ended.
  startedAt: string | null
  finishedAt: string | null
}

// A run started, or a person retried, skipped, or resumed one of its steps.
export type RunHistoryEntry = {
  id: string
  runId: string
  stepJobId: string | null
  kind: 'started' | 'retried' | 'skipped' | 'resumed'
  createdBy: string
  data: Record<string, unknown>
  createdAt: string
}

type ListRunsResponse = {
  runs: WorkflowRun[]
  hasMore: boolean
}

// A page of runs, newest first, older than run `before` when it is given.
export function listRuns(before: string | null) {
  const searchParams = before
    ? { before }
    : undefined
  return apiClient
    .get('v1/runs', { searchParams })
    .json<ListRunsResponse>()
}

export type RunDetail = {
  run: WorkflowRun
  steps: RunStep[]
  history: RunHistoryEntry[]
}

export function getRun(runId: string) {
  return apiClient
    .get(`v1/runs/${runId}`)
    .json<RunDetail>()
}

type StepLogResponse = {
  log: string
}

export function getStepLog(runId: string, stepId: string) {
  return apiClient
    .get(`v1/runs/${runId}/steps/${stepId}/log`)
    .json<StepLogResponse>()
}

// What a person may do to a step, each answering the run as it now stands.
export const STEP_ACTIONS = [ 'retry', 'skip', 'resume' ] as const
export type StepAction = typeof STEP_ACTIONS[number]

export function actOnStep(runId: string, stepId: string, action: StepAction) {
  return apiClient
    .post(`v1/runs/${runId}/steps/${stepId}/${action}`)
    .json<RunDetail>()
}
