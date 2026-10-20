// Copyright © 2026 Jalapeno Labs

// Redux
import { listenerMiddleware, store } from '../store'
import { elysiaPanelToggled, selectIsElysiaPanelOpen } from '../store/elysiaSlice'

// Misc
import { ELYSIA_PANEL_STORAGE_KEY } from '../constants'

// Remembers whether Elysia's panel is open, per browser. Runs once at startup.
export function startElysiaPanelSync() {
  listenerMiddleware.startListening({
    actionCreator: elysiaPanelToggled,
    effect: () => {
      const isOpen = selectIsElysiaPanelOpen(store.getState())
      try {
        window.localStorage.setItem(
          ELYSIA_PANEL_STORAGE_KEY,
          isOpen
            ? 'open'
            : 'closed',
        )
      }
      catch (error) {
        console.debug('Could not remember whether the Elysia panel is open', { error })
      }
    },
  })
}
