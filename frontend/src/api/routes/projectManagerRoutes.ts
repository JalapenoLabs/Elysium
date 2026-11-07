// Copyright © 2026 Jalapeno Labs

import type { LinkProvider } from './actionItemRoutes'
import type { PersonPromise } from './promiseRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `Status` in api/src/project_managers/status.rs.
export const PROJECT_MANAGER_STATUSES = [ 'running', 'held', 'errored', 'waiting', 'idle' ] as const
export type ProjectManagerStatus = typeof PROJECT_MANAGER_STATUSES[number]

// Mirrors `TurnStatus` in api/src/models/project_manager.rs.
export const PROJECT_MANAGER_TURN_STATUSES = [ 'running', 'succeeded', 'failed' ] as const
export type ProjectManagerTurnStatus = typeof PROJECT_MANAGER_TURN_STATUSES[number]

// Mirrors `WakeKind` in api/src/models/project_manager.rs.
export const PROJECT_MANAGER_WAKE_KINDS = [ 'record', 'batch', 'message', 'promise' ] as const
export type ProjectManagerWakeKind = typeof PROJECT_MANAGER_WAKE_KINDS[number]

// Mirrors `Priority` in api/src/models/work_batch.rs.
export const WORK_BATCH_PRIORITIES = [ 'asap', 'normal', 'scheduled' ] as const
export type WorkBatchPriority = typeof WORK_BATCH_PRIORITIES[number]

// Mirrors `BatchState` in api/src/models/work_batch.rs.
export const WORK_BATCH_STATES = [
  // Its Project Manager proposed it; it waits for a person to approve it before it may queue.
  'proposed',
  'queued',
  'paused',
  'assigned',
  'running',
  'succeeded',
  'failed',
  'cancelled',
  // A person declined it while it was proposed, saying why.
  'rejected',
] as const
export type WorkBatchState = typeof WORK_BATCH_STATES[number]

// Mirrors `Wait` in api/src/models/work_batch/worker.rs: what a running batch's worker waits on
// while it keeps its container.
export const WORK_BATCH_WAITS = [ 'review', 'ci', 'question' ] as const
export type WorkBatchWait = typeof WORK_BATCH_WAITS[number]

// Mirrors `Origin` in api/src/models/work_batch/lineage.rs: whether a batch traces to a person or
// only to what an agent noticed.
export const WORK_BATCH_ORIGINS = [ 'human_rooted', 'found' ] as const
export type WorkBatchOrigin = typeof WORK_BATCH_ORIGINS[number]

// One thing that woke a turn: a message with its text, a record, a batch, or a promise, by id.
export type ProjectManagerWake = {
  id: string
  kind: ProjectManagerWakeKind
  timelineRecordId: string | null
  workBatchId: string | null
  promiseId: string | null
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
  projectId: string
  title: string
  goal: string
  repositories: WorkBatchRepository[]
  priority: WorkBatchPriority
  // When a scheduled batch is due.
  scheduledFor: string | null
  state: WorkBatchState
  // What its worker reported, or why it was cancelled or rejected.
  result: string | null
  // The turn that submitted it.
  turnId: string | null
  sourceRecordIds: string[]
  actionItemIds: string[]
  // The messages to its Project Manager that called for it.
  sourceMessageIds: string[]
  origin: WorkBatchOrigin
  // How many batches led to it: 0 for work a person called for.
  generation: number
  // The batches it was planned from.
  parentBatchIds: string[]
  timelineRecordId: string | null
  // While someone asked for it to run next.
  pinnedAt: string | null
  // When the pool gave it a slot.
  assignedAt: string | null
  createdAt: string
  startedAt: string | null
  finishedAt: string | null
  // Set only while a running batch's worker keeps its container waiting: on what, in its
  // worker's words, and when it looks again unless a question is answered first.
  waitingOn: WorkBatchWait | null
  waitingDetail: string | null
  waitingUntil: string | null
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
  // Every batch of its waiting for a person's approval, newest first, up to 200.
  proposed: WorkBatch[]
  // Its latest promises, newest first.
  promises: PersonPromise[]
  // How its changes reach the work.
  writes: ProjectManagerWrites
}

// Mirrors `TrackerSource` in api/src/project_managers/trackers.rs: the project names its
// tracker, or follows where most of its items' issues already are.
export const TRACKER_SOURCES = [ 'named', 'precedent' ] as const
export type TrackerSource = typeof TRACKER_SOURCES[number]

// Mirrors `TrackerResponse` in api/src/routes/v1/projects/project_manager/mod.rs.
export type ProjectTracker = {
  source: TrackerSource
  provider: LinkProvider
  // The Jira credential, GitHub token, or YouTrack account that files there.
  credentialId: string
  credentialName: string
  // A Jira project key, a GitHub owner/name, or a YouTrack project's short name.
  destination: string
}

// Mirrors `WritesResponse`: whether its changes apply as it makes them, and where its issues
// file, or why there is nowhere.
export type ProjectManagerWrites = {
  direct: boolean
  tracker: ProjectTracker | null
  trackerRefusal: string | null
}

// A place issues could file, as its provider names it.
export type TrackerDestination = {
  reference: string
  name: string
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

// Whether its changes apply as it makes them, rather than waiting for a person's approval.
export function setProjectManagerWritesDirectly(projectId: string, writesDirectly: boolean) {
  return apiClient
    .patch(getProjectManagerKey(projectId), { json: { writesDirectly }})
    .json<ProjectManagerResponse>()
}

type SetProjectTrackerRequest = {
  provider: LinkProvider
  // The Jira credential or YouTrack account that files there; left out for GitHub, which files
  // with the project's own token.
  credentialId?: string
  destination: string
}

// Names the tracker the project's issues file in. The place is read through its credential
// first, so one the credential cannot reach is refused.
export function setProjectTracker(projectId: string, body: SetProjectTrackerRequest) {
  return apiClient
    .put(`${getProjectManagerKey(projectId)}/tracker`, { json: body })
    .json<ProjectManagerResponse>()
}

// Stops naming a tracker, so issues file where most of the project's issues already are.
export function clearProjectTracker(projectId: string) {
  return apiClient
    .delete(`${getProjectManagerKey(projectId)}/tracker`)
    .json<ProjectManagerResponse>()
}

export function getTrackerDestinationsKey(
  projectId: string,
  provider: LinkProvider,
  credentialId: string | null,
) {
  return [ 'tracker-destinations', projectId, provider, credentialId ?? '' ] as const
}

type TrackerDestinationsResponse = {
  destinations: TrackerDestination[]
}

// The places a tracker on `provider` could file the project's issues, read through the
// credential that would file there; for GitHub, the project's own token.
export function listTrackerDestinations(
  projectId: string,
  provider: LinkProvider,
  credentialId: string | null,
) {
  const searchParams: Record<string, string> = { provider }
  if (credentialId) {
    searchParams.credentialId = credentialId
  }
  return apiClient
    .get(`${getProjectManagerKey(projectId)}/destinations`, { searchParams })
    .json<TrackerDestinationsResponse>()
}

export function sendProjectManagerMessage(projectId: string, body: string) {
  return apiClient
    .post(`${getProjectManagerKey(projectId)}/messages`, { json: { body }})
    .json<ProjectManagerResponse>()
}
