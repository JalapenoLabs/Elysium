// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

// Misc
import { CODING_SESSIONS_VIEW_STORAGE_KEY } from '../../constants'
import { readStoredSessionsView, saveSessionsView } from './sessionsView'

beforeEach(() => {
  window.localStorage.clear()
  // Every fallback logs why; keep the output quiet.
  vi.spyOn(console, 'debug').mockImplementation(() => {})
})

afterEach(() => {
  vi.restoreAllMocks()
})

describe('readStoredSessionsView', () => {
  it('shows the table when nothing is stored', () => {
    expect(readStoredSessionsView()).toBe('table')
  })

  it('reads a stored view back', () => {
    window.localStorage.setItem(CODING_SESSIONS_VIEW_STORAGE_KEY, 'tiles')
    expect(readStoredSessionsView()).toBe('tiles')
  })

  it('falls back to the table for a value it does not know', () => {
    window.localStorage.setItem(CODING_SESSIONS_VIEW_STORAGE_KEY, 'cards')
    expect(readStoredSessionsView()).toBe('table')
  })

  it('falls back to the table when storage cannot be read', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('Storage is disabled')
    })
    expect(readStoredSessionsView()).toBe('table')
  })
})

describe('saveSessionsView', () => {
  it('stores the view under its key', () => {
    saveSessionsView('tiles')
    expect(window.localStorage.getItem(CODING_SESSIONS_VIEW_STORAGE_KEY)).toBe('tiles')
  })

  it('keeps going when storage cannot be written', () => {
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('Storage is full')
    })
    expect(() => saveSessionsView('tiles')).not.toThrow()
  })
})
