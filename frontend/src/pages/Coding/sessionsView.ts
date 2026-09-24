// Copyright © 2026 Jalapeno Labs

// Misc
import { CODING_SESSIONS_VIEW_STORAGE_KEY } from '../../constants'

// Whether the Sessions panel shows a table or tiles is a per-browser choice stored in
// localStorage and held in Redux (`sessionsViewSlice`), the way the theme is. These are
// the storage half; `startSessionsViewSync` wires them to the store.

export const SESSIONS_VIEWS = [ 'table', 'tiles' ] as const
export type SessionsView = typeof SESSIONS_VIEWS[number]

const DEFAULT_SESSIONS_VIEW: SessionsView = 'table'

export function readStoredSessionsView(): SessionsView {
  try {
    const stored = window.localStorage.getItem(CODING_SESSIONS_VIEW_STORAGE_KEY)
    const match = SESSIONS_VIEWS.find((view) => view === stored)
    if (match) {
      return match
    }
    if (stored !== null) {
      console.debug('Ignoring unknown stored sessions view', { stored })
    }
  }
  catch (error) {
    // Storage can be unavailable (privacy modes); the default still works.
    console.debug('The sessions view could not be read from localStorage', { error })
  }
  return DEFAULT_SESSIONS_VIEW
}

export function saveSessionsView(view: SessionsView) {
  try {
    window.localStorage.setItem(CODING_SESSIONS_VIEW_STORAGE_KEY, view)
  }
  catch (error) {
    console.debug('The sessions view could not be saved to localStorage', { error })
  }
}
