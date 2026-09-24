// Copyright © 2026 Jalapeno Labs

// Redux
import { listenerMiddleware, store } from '../../store'
import { sessionsViewChanged } from '../../store/sessionsViewSlice'

// Misc
import { CODING_SESSIONS_VIEW_STORAGE_KEY } from '../../constants'
import { readStoredSessionsView, saveSessionsView } from './sessionsView'

// Connects the sessions view slice to localStorage. Runs once at startup: saves every
// change, and follows the choice made in another tab.
export function startSessionsViewSync() {
  listenerMiddleware.startListening({
    actionCreator: sessionsViewChanged,
    effect: (action) => {
      saveSessionsView(action.payload)
    },
  })

  window.addEventListener('storage', (event) => {
    if (event.key !== CODING_SESSIONS_VIEW_STORAGE_KEY) {
      return
    }
    store.dispatch(sessionsViewChanged(readStoredSessionsView()))
  })
}
