// Copyright © 2026 Jalapeno Labs

import type { ThemePreference } from '@jalapenolabs/uikit'

// The theme preference is a per-browser choice stored in localStorage and held in
// Redux (`themeSlice`). These are the storage and DOM halves: reading and saving the
// stored value, resolving "system" against the OS setting, and applying the result
// to <html>. `startThemeSync` wires them to the store.
//
// index.html runs a tiny inline copy of the read-and-apply step before the CSS
// loads so the first paint already has the right theme. Keep the two in sync.

export type ResolvedTheme = 'light' | 'dark'

export const THEME_PREFERENCES = [ 'light', 'dark', 'system' ] as const satisfies readonly ThemePreference[]

// Shared with the inline script in index.html.
export const THEME_STORAGE_KEY = 'elysium.theme'
const DEFAULT_PREFERENCE: ThemePreference = 'system'

export const darkColorSchemeMedia = window.matchMedia('(prefers-color-scheme: dark)')

export function readStoredThemePreference(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(THEME_STORAGE_KEY)
    const match = THEME_PREFERENCES.find((preference) => preference === stored)
    if (match) {
      return match
    }
    if (stored !== null) {
      console.debug('Ignoring unknown stored theme preference', { stored })
    }
  }
  catch (error) {
    // Storage can be unavailable (privacy modes); the default still works.
    console.debug('Theme preference could not be read from localStorage', { error })
  }
  return DEFAULT_PREFERENCE
}

export function saveThemePreference(preference: ThemePreference) {
  try {
    window.localStorage.setItem(THEME_STORAGE_KEY, preference)
  }
  catch (error) {
    console.debug('Theme preference could not be saved to localStorage', { error })
  }
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference !== 'system') {
    return preference
  }

  if (darkColorSchemeMedia.matches) {
    return 'dark'
  }
  return 'light'
}

// HeroUI reads the `light`/`dark` class and `data-theme`; `color-scheme` makes
// native controls and scrollbars follow along.
export function applyTheme(theme: ResolvedTheme) {
  const root = document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(theme)
  root.dataset.theme = theme
  root.style.colorScheme = theme
}
