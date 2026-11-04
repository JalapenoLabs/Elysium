// Copyright © 2026 Jalapeno Labs

// Redux
import { listenerMiddleware, store } from '../store'
import { sidebarCollapsedChanged } from '../store/sidebarSlice'

// Misc
import { SIDEBAR_STORAGE_KEY } from '../constants'
import { readStoredSidebarCollapsed, saveSidebarCollapsed } from './sidebarCollapse'

// Connects the sidebar slice to localStorage. Runs once at startup: saves every change, and
// follows the choice made in another tab.
export function startSidebarSync() {
  listenerMiddleware.startListening({
    actionCreator: sidebarCollapsedChanged,
    effect: (action) => {
      saveSidebarCollapsed(action.payload)
    },
  })

  window.addEventListener('storage', (event) => {
    if (event.key !== SIDEBAR_STORAGE_KEY) {
      return
    }
    store.dispatch(sidebarCollapsedChanged(readStoredSidebarCollapsed()))
  })
}
