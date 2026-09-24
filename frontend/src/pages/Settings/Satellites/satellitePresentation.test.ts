// Copyright © 2026 Jalapeno Labs

import type { Satellite, SatelliteSetupStatus } from '../../../api/routes/satelliteRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getSatelliteSetupDisplay } from './satellitePresentation'

function makeSatellite(setup: SatelliteSetupStatus | null, isActive = true): Satellite {
  return {
    id: 'satellite',
    name: 'Local',
    description: '',
    url: 'http://172.17.0.1:8090',
    isActive,
    createdAt: '2026-09-24T00:00:00Z',
    updatedAt: '2026-09-24T00:00:00Z',
    status: {
      satelliteId: 'satellite',
      reachable: setup !== null,
      version: '1.0.0',
      runningThreads: 0,
      maxConcurrentThreads: 4,
      error: null,
      setup,
    },
  }
}

describe('getSatelliteSetupDisplay', () => {
  it('maps each state of Elysium\'s own script', () => {
    const states: SatelliteSetupStatus['state'][] = [ 'none', 'running', 'succeeded', 'failed', 'unknown' ]
    const displays = states.map((state) => getSatelliteSetupDisplay(
      makeSatellite({ state, isCurrent: true, failureOutput: null }),
    ))
    expect(displays).toEqual([ 'pending', 'installing', 'ready', 'failed', 'pending' ])
  })

  it('shows a script Elysium is replacing as pending, whatever its state', () => {
    const satellite = makeSatellite({ state: 'succeeded', isCurrent: false, failureOutput: null })
    expect(getSatelliteSetupDisplay(satellite)).toBe('pending')
  })

  it('shows nothing for an unreachable or inactive satellite', () => {
    expect(getSatelliteSetupDisplay(makeSatellite(null))).toBeNull()

    const inactive = makeSatellite({ state: 'succeeded', isCurrent: true, failureOutput: null }, false)
    expect(getSatelliteSetupDisplay(inactive)).toBeNull()
  })
})
