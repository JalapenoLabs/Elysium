// Copyright © 2026 Jalapeno Labs

import type { UpdatesStatus } from '../../../api/routes/updateRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { isUpdaterMissing, releaseInQuestion } from './updatesPresentation'

function makeStatus(overrides: Partial<UpdatesStatus> = {}): UpdatesStatus {
  return {
    release: '0.4.2',
    commit: '3fb90adcd6c4',
    builtAt: '2026-10-20T08:00:00Z',
    state: 'up-to-date',
    target: null,
    newestRelease: '0.4.2',
    lastCheckedAt: '2026-10-20T09:00:00Z',
    lastCheckError: null,
    updaterConnected: true,
    updaterSeenAt: '2026-10-20T09:05:00Z',
    waitingFor: [],
    history: [],
    ...overrides,
  }
}

describe('releaseInQuestion', () => {
  it('names the release that is ready or waiting', () => {
    const status = makeStatus({ state: 'waiting-for-work', target: '0.4.3', newestRelease: '0.4.3' })
    expect(releaseInQuestion(status)).toBe('0.4.3')
  })

  it('names the newest release when it is held back, since nothing is targeted', () => {
    const status = makeStatus({ state: 'held-back', target: null, newestRelease: '0.4.3' })
    expect(releaseInQuestion(status)).toBe('0.4.3')
  })
})

describe('isUpdaterMissing', () => {
  it('flags a release deployment whose updater is not connected', () => {
    expect(isUpdaterMissing(makeStatus({ updaterConnected: false }))).toBe(true)
    expect(isUpdaterMissing(makeStatus({ updaterConnected: true }))).toBe(false)
  })

  it('never flags a build that does not update itself', () => {
    const status = makeStatus({ state: 'not-a-release', release: null, updaterConnected: false })
    expect(isUpdaterMissing(status)).toBe(false)
  })
})
