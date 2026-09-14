// Copyright © 2026 Jalapeno Labs

import type { ThemePreference } from '@jalapenolabs/uikit'

// Theme preference is a per-browser choice stored in localStorage. This module is
// the single owner of it: it reads and writes the stored value, resolves "system"
// against the OS setting, applies the result to <html>, and lets React subscribe.
//
// index.html runs a tiny inline copy of the read-and-apply step before the CSS
// loads so the first paint already has the right theme. Keep the two in sync.

export type ResolvedTheme = 'light' | 'dark'

export const THEME_PREFERENCES = [ 'light', 'dark', 'system' ] as const satisfies readonly ThemePreference[]

// Shared with the inline script in index.html.
const STORAGE_KEY = 'elysium.theme'
const DEFAULT_PREFERENCE: ThemePreference = 'system'
const DARK_MEDIA_QUERY = '(prefers-color-scheme: dark)'

const listeners = new Set<() => void>()
const darkMedia = window.matchMedia(DARK_MEDIA_QUERY)
let currentPreference = readStoredPreference()

function readStoredPreference(): ThemePreference {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
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

function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference !== 'system') {
    return preference
  }

  if (darkMedia.matches) {
    return 'dark'
  }
  return 'light'
}

// HeroUI reads the `light`/`dark` class and `data-theme`; `color-scheme` makes
// native controls and scrollbars follow along.
function applyTheme(theme: ResolvedTheme) {
  const root = document.documentElement
  root.classList.remove('light', 'dark')
  root.classList.add(theme)
  root.dataset.theme = theme
  root.style.colorScheme = theme
}

function notify() {
  for (const listener of listeners) {
    listener()
  }
}

export function getThemePreference(): ThemePreference {
  return currentPreference
}

export function setThemePreference(preference: ThemePreference) {
  currentPreference = preference
  try {
    window.localStorage.setItem(STORAGE_KEY, preference)
  }
  catch (error) {
    console.debug('Theme preference could not be saved to localStorage', { error })
  }
  applyTheme(resolveTheme(preference))
  notify()
}

export function subscribeToThemePreference(listener: () => void) {
  listeners.add(listener)
  return () => {
    listeners.delete(listener)
  }
}

// Runs once at startup. Re-applies on OS theme changes while "system" is chosen,
// and on changes made in another tab.
export function initializeTheme() {
  applyTheme(resolveTheme(currentPreference))

  darkMedia.addEventListener('change', () => {
    if (currentPreference === 'system') {
      applyTheme(resolveTheme(currentPreference))
    }
  })

  window.addEventListener('storage', (event) => {
    if (event.key !== STORAGE_KEY) {
      return
    }
    currentPreference = readStoredPreference()
    applyTheme(resolveTheme(currentPreference))
    notify()
  })
}
