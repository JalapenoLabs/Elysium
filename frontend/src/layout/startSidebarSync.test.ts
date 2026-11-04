// Copyright © 2026 Jalapeno Labs

// Core
import { beforeAll, beforeEach, describe, expect, it } from 'vitest'

// Redux
import { store } from '../store'
import { selectIsSidebarCollapsed, sidebarCollapsedChanged } from '../store/sidebarSlice'

// Misc
import { SIDEBAR_STORAGE_KEY } from '../constants'
import { startSidebarSync } from './startSidebarSync'

// The sync wires the app's one store, as `main.tsx` does, so it starts once for the file.
beforeAll(() => {
  startSidebarSync()
})

beforeEach(() => {
  store.dispatch(sidebarCollapsedChanged(false))
  window.localStorage.clear()
})

describe('startSidebarSync', () => {
  it('remembers every collapse and expand', () => {
    store.dispatch(sidebarCollapsedChanged(true))
    expect(window.localStorage.getItem(SIDEBAR_STORAGE_KEY)).toBe('collapsed')

    store.dispatch(sidebarCollapsedChanged(false))
    expect(window.localStorage.getItem(SIDEBAR_STORAGE_KEY)).toBe('expanded')
  })

  it('follows a choice made in another tab', () => {
    // Another tab has saved its choice; the browser tells this one through a storage event.
    window.localStorage.setItem(SIDEBAR_STORAGE_KEY, 'collapsed')
    window.dispatchEvent(new StorageEvent('storage', { key: SIDEBAR_STORAGE_KEY }))

    expect(selectIsSidebarCollapsed(store.getState())).toBe(true)
  })

  it('ignores storage events for other keys', () => {
    window.localStorage.setItem(SIDEBAR_STORAGE_KEY, 'collapsed')
    window.dispatchEvent(new StorageEvent('storage', { key: 'elysium.theme' }))

    expect(selectIsSidebarCollapsed(store.getState())).toBe(false)
  })
})
