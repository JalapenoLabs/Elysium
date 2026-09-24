// Copyright © 2026 Jalapeno Labs

// Core
import { beforeAll, beforeEach, describe, expect, it } from 'vitest'

// Redux
import { store } from '../../store'
import { selectSessionsView, sessionsViewChanged } from '../../store/sessionsViewSlice'

// Misc
import { CODING_SESSIONS_VIEW_STORAGE_KEY } from '../../constants'
import { startSessionsViewSync } from './startSessionsViewSync'

// The sync wires the app's one store, as `main.tsx` does, so it starts once for the file.
beforeAll(() => {
  startSessionsViewSync()
})

beforeEach(() => {
  store.dispatch(sessionsViewChanged('table'))
  window.localStorage.clear()
})

describe('startSessionsViewSync', () => {
  it('saves every view chosen', () => {
    store.dispatch(sessionsViewChanged('tiles'))

    expect(selectSessionsView(store.getState())).toBe('tiles')
    expect(window.localStorage.getItem(CODING_SESSIONS_VIEW_STORAGE_KEY)).toBe('tiles')
  })

  it('follows a view chosen in another tab', () => {
    // Another tab has saved its choice; the browser tells this one through a storage event.
    window.localStorage.setItem(CODING_SESSIONS_VIEW_STORAGE_KEY, 'tiles')
    window.dispatchEvent(new StorageEvent('storage', { key: CODING_SESSIONS_VIEW_STORAGE_KEY }))

    expect(selectSessionsView(store.getState())).toBe('tiles')
  })

  it('ignores storage events for other keys', () => {
    window.localStorage.setItem(CODING_SESSIONS_VIEW_STORAGE_KEY, 'tiles')
    window.dispatchEvent(new StorageEvent('storage', { key: 'elysium.theme' }))

    expect(selectSessionsView(store.getState())).toBe('table')
  })
})
