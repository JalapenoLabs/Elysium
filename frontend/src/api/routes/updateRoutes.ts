// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `UpdateState` in api/src/updates/decision.rs.
export type UpdateState =
  | 'not-a-release'
  | 'up-to-date'
  | 'held-back'
  | 'waiting-for-work'
  | 'applying'
  | 'ready'

// Mirrors `DeploymentUpdateStatus` in api/src/models/deployment_update.rs.
export type DeploymentUpdateStatus = 'applying' | 'succeeded' | 'rolled-back'

// Mirrors `Activity` in api/src/activity.rs.
export type Activity = {
  kind: 'agentTurn'
  label: string
  startedAt: string
}

// Mirrors `DeploymentUpdateView` in api/src/updates/mod.rs.
export type DeploymentUpdate = {
  id: string
  fromVersion: string
  toVersion: string
  status: DeploymentUpdateStatus
  reason: string | null
  startedAt: string
  finishedAt: string | null
  createdBy: string
}

// Mirrors `UpdatesStatus` in api/src/updates/mod.rs: the release running, auto-update's
// state, and the update history, newest first.
export type UpdatesStatus = {
  release: string | null
  commit: string | null
  builtAt: string
  state: UpdateState
  target: string | null
  newestRelease: string | null
  lastCheckedAt: string | null
  lastCheckError: string | null
  updaterConnected: boolean
  updaterSeenAt: string | null
  waitingFor: Activity[]
  history: DeploymentUpdate[]
}

type UpdatesResponse = {
  updates: UpdatesStatus
}

export function getUpdates() {
  return apiClient
    .get('v1/updates')
    .json<UpdatesResponse>()
}

export function checkForUpdates() {
  return apiClient
    .post('v1/updates/check')
    .json<UpdatesResponse>()
}

// Asks for the newest release, even one that rolled back before. It applies once nothing
// is running that a restart would cut short.
export function applyNewestRelease() {
  return apiClient
    .post('v1/updates/apply')
    .json<UpdatesResponse>()
}
