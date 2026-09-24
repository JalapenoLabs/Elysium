// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Satellite, SatelliteSetupStatus } from '../../../api/routes/satelliteRoutes'

export type SatelliteHealth = 'online' | 'offline' | 'checking' | 'inactive'

export const satelliteHealthLabelKeys = {
  online: 'status.online',
  offline: 'status.offline',
  checking: 'status.checking',
  inactive: 'status.inactive',
} as const satisfies Record<SatelliteHealth, ParseKeys<'satellites'>>

export const satelliteHealthChipColors = {
  online: 'success',
  offline: 'danger',
  checking: 'default',
  inactive: 'default',
} as const satisfies Record<SatelliteHealth, 'success' | 'danger' | 'default'>

// Inactive outranks the last poll: the API stops watching an inactive satellite, so
// any status it still carries is stale.
export function getSatelliteHealth(satellite: Satellite): SatelliteHealth {
  if (!satellite.isActive) {
    return 'inactive'
  }
  if (!satellite.status) {
    return 'checking'
  }
  if (satellite.status.reachable) {
    return 'online'
  }
  return 'offline'
}

// What a satellite's setup (the Blender install) looks like to the user. A script that is not
// Elysium's own is about to be replaced, so it reads as pending whatever its state.
export type SatelliteSetupDisplay = 'pending' | 'installing' | 'ready' | 'failed'

export const satelliteSetupLabelKeys = {
  pending: 'setup.pending',
  installing: 'setup.installing',
  ready: 'setup.ready',
  failed: 'setup.failed',
} as const satisfies Record<SatelliteSetupDisplay, ParseKeys<'satellites'>>

export const satelliteSetupChipColors = {
  pending: 'default',
  installing: 'warning',
  ready: 'success',
  failed: 'danger',
} as const satisfies Record<SatelliteSetupDisplay, 'success' | 'warning' | 'danger' | 'default'>

const setupDisplayByState = {
  unknown: 'pending',
  none: 'pending',
  running: 'installing',
  succeeded: 'ready',
  failed: 'failed',
} as const satisfies Record<SatelliteSetupStatus['state'], SatelliteSetupDisplay>

// Null when there is nothing current to show: an inactive or unreachable satellite.
export function getSatelliteSetupDisplay(satellite: Satellite): SatelliteSetupDisplay | null {
  const setup = satellite.status?.setup
  if (!satellite.isActive || !setup) {
    return null
  }
  if (!setup.isCurrent) {
    return 'pending'
  }
  return setupDisplayByState[setup.state]
}
