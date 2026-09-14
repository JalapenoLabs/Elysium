// Copyright © 2026 Jalapeno Labs

// Redux
import { listenerMiddleware, store } from '../store'
import { themeChanged } from '../store/themeSlice'

// Misc
import {
  applyTheme,
  darkColorSchemeMedia,
  readStoredThemePreference,
  saveThemePreference,
  THEME_STORAGE_KEY,
} from './themePreference'

// Connects the theme slice to the page. Runs once at startup: applies the stored
// theme, saves and applies every change, and follows the OS color scheme and other
// tabs' choices.
export function startThemeSync() {
  applyTheme(store.getState().theme.resolved)

  listenerMiddleware.startListening({
    actionCreator: themeChanged,
    effect: (action) => {
      saveThemePreference(action.payload.preference)
      applyTheme(action.payload.resolved)
    },
  })

  darkColorSchemeMedia.addEventListener('change', () => {
    const preference = store.getState().theme.preference
    if (preference === 'system') {
      store.dispatch(themeChanged(preference))
    }
  })

  window.addEventListener('storage', (event) => {
    if (event.key !== THEME_STORAGE_KEY) {
      return
    }
    store.dispatch(themeChanged(readStoredThemePreference()))
  })
}
