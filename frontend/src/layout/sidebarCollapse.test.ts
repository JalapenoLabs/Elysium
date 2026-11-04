// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { SIDEBAR_STORAGE_KEY } from '../constants'
import { readStoredSidebarCollapsed, saveSidebarCollapsed } from './sidebarCollapse'

beforeEach(() => {
  window.localStorage.clear()
  // Every fallback logs why; keep the output quiet.
  vi.spyOn(console, 'debug').mockImplementation(() => {})
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('readStoredSidebarCollapsed', () => {
  it('starts expanded when nothing is stored', () => {
    expect(readStoredSidebarCollapsed()).toBe(false)
  })

  it('reads a stored choice back', () => {
    saveSidebarCollapsed(true)
    expect(readStoredSidebarCollapsed()).toBe(true)

    saveSidebarCollapsed(false)
    expect(readStoredSidebarCollapsed()).toBe(false)
  })

  it('starts expanded for a value it does not know', () => {
    window.localStorage.setItem(SIDEBAR_STORAGE_KEY, 'true')
    expect(readStoredSidebarCollapsed()).toBe(false)
  })

  it('starts expanded when storage cannot be read', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('Storage is disabled')
    })
    expect(readStoredSidebarCollapsed()).toBe(false)
  })
})

describe('saveSidebarCollapsed', () => {
  it('stores the choice under its key', () => {
    saveSidebarCollapsed(true)
    expect(window.localStorage.getItem(SIDEBAR_STORAGE_KEY)).toBe('collapsed')
  })

  it('keeps going when storage cannot be written', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('Storage is full')
    })
    expect(() => saveSidebarCollapsed(true)).not.toThrow()
  })
})
