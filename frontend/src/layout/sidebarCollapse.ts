// Copyright © 2026 Jalapeno Labs

// Misc
import { SIDEBAR_STORAGE_KEY } from '../constants'

// Whether the sidebar is collapsed to its icons is a per-browser choice stored in
// localStorage and held in Redux (`sidebarSlice`), the way the theme is. These are the
// storage half; `startSidebarSync` wires them to the store.

const COLLAPSED = 'collapsed'
const EXPANDED = 'expanded'

// The sidebar starts expanded, so a first visit shows every label.
export function readStoredSidebarCollapsed() {
  try {
    const stored = window.localStorage.getItem(SIDEBAR_STORAGE_KEY)
    if (stored === COLLAPSED) {
      return true
    }
    if (stored !== null && stored !== EXPANDED) {
      console.debug('Ignoring unknown stored sidebar state', { stored })
    }
  }
  catch (error) {
    // Storage can be unavailable (privacy modes); the sidebar still works, expanded.
    console.debug('The sidebar state could not be read from localStorage', { error })
  }
  return false
}

export function saveSidebarCollapsed(isCollapsed: boolean) {
  try {
    window.localStorage.setItem(
      SIDEBAR_STORAGE_KEY,
      isCollapsed
        ? COLLAPSED
        : EXPANDED,
    )
  }
  catch (error) {
    console.debug('The sidebar state could not be saved to localStorage', { error })
  }
}
