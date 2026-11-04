// Copyright © 2026 Jalapeno Labs

import type { UpdatesStatus } from '../api/routes/updateRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Redux
import { createAppStore } from './index'
import { updatesStatusUpdated } from './updatesSlice'
import { buildRead, refreshPromptDismissed, selectIsRefreshOffered } from './versionSlice'

const loaded = '2026-11-04T00:00:00Z'
const next = '2026-11-05T00:00:00Z'
const later = '2026-11-06T00:00:00Z'

function statusBuiltAt(builtAt: string): UpdatesStatus {
  return {
    release: '1.4.2',
    commit: 'abc1234',
    builtAt,
    state: 'up-to-date',
    target: null,
    newestRelease: '1.4.2',
    lastCheckedAt: null,
    lastCheckError: null,
    updaterConnected: true,
    updaterSeenAt: null,
    waitingFor: [],
    history: [],
  }
}

describe('selectIsRefreshOffered', () => {
  it('offers nothing while the API runs the build this page loaded', () => {
    const store = createAppStore()
    expect(selectIsRefreshOffered(store.getState())).toBe(false)

    store.dispatch(buildRead(loaded))
    store.dispatch(buildRead(loaded))
    expect(selectIsRefreshOffered(store.getState())).toBe(false)
  })

  it('offers a refresh once another build runs, and withdraws it if the loaded one returns', () => {
    const store = createAppStore()
    store.dispatch(buildRead(loaded))

    store.dispatch(buildRead(next))
    expect(selectIsRefreshOffered(store.getState())).toBe(true)

    store.dispatch(buildRead(loaded))
    expect(selectIsRefreshOffered(store.getState())).toBe(false)
  })

  it('stays dismissed for that build, and offers again for a later one', () => {
    const store = createAppStore()
    store.dispatch(buildRead(loaded))
    store.dispatch(buildRead(next))

    store.dispatch(refreshPromptDismissed())
    expect(selectIsRefreshOffered(store.getState())).toBe(false)
    store.dispatch(buildRead(next))
    expect(selectIsRefreshOffered(store.getState())).toBe(false)

    store.dispatch(buildRead(later))
    expect(selectIsRefreshOffered(store.getState())).toBe(true)
  })

  it('follows the build auto-update reports, once the loaded build is known', () => {
    const store = createAppStore()
    store.dispatch(updatesStatusUpdated(statusBuiltAt(next)))
    expect(store.getState().version.runningBuild).toBeNull()

    store.dispatch(buildRead(loaded))
    store.dispatch(updatesStatusUpdated(statusBuiltAt(next)))
    expect(selectIsRefreshOffered(store.getState())).toBe(true)
  })
})
