// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { Satellite } from '../../../api/routes/satelliteRoutes'

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
