// Copyright © 2026 Jalapeno Labs

// Core
import { useSyncExternalStore } from 'react'

// Misc
import {
  getThemePreference,
  setThemePreference,
  subscribeToThemePreference,
} from '../theme/themePreference'

export function useThemePreference() {
  const preference = useSyncExternalStore(subscribeToThemePreference, getThemePreference)

  return {
    preference,
    setPreference: setThemePreference,
  } as const
}
