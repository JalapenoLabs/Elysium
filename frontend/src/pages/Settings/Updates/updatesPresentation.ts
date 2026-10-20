// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { DeploymentUpdateStatus, UpdatesStatus, UpdateState } from '../../../api/routes/updateRoutes'

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const stateLabelKeys = {
  'not-a-release': 'state.notARelease.label',
  'up-to-date': 'state.upToDate.label',
  'held-back': 'state.heldBack.label',
  'waiting-for-work': 'state.waitingForWork.label',
  'applying': 'state.applying.label',
  'ready': 'state.ready.label',
} as const satisfies Record<UpdateState, ParseKeys<'updates'>>

export const stateDescriptionKeys = {
  'not-a-release': 'state.notARelease.description',
  'up-to-date': 'state.upToDate.description',
  'held-back': 'state.heldBack.description',
  'waiting-for-work': 'state.waitingForWork.description',
  'applying': 'state.applying.description',
  'ready': 'state.ready.description',
} as const satisfies Record<UpdateState, ParseKeys<'updates'>>

export const stateChipColors = {
  'not-a-release': 'default',
  'up-to-date': 'success',
  'held-back': 'warning',
  'waiting-for-work': 'accent',
  'applying': 'accent',
  'ready': 'accent',
} as const satisfies Record<UpdateState, ChipColor>

export const historyStatusLabelKeys = {
  'applying': 'history.statuses.applying',
  'succeeded': 'history.statuses.succeeded',
  'rolled-back': 'history.statuses.rolledBack',
} as const satisfies Record<DeploymentUpdateStatus, ParseKeys<'updates'>>

export const historyStatusChipColors = {
  'applying': 'accent',
  'succeeded': 'success',
  'rolled-back': 'danger',
} as const satisfies Record<DeploymentUpdateStatus, ChipColor>

// The release a state's description names: the one waiting or ready, or, when it is held
// back, the newest one, which rolled back here before.
export function releaseInQuestion(status: UpdatesStatus) {
  return status.target ?? status.newestRelease
}

// Whether the updater is missing where it matters: a release deployment that nothing is
// updating. A development build never updates itself, so its updater is beside the point.
export function isUpdaterMissing(status: UpdatesStatus) {
  return status.state !== 'not-a-release' && !status.updaterConnected
}
