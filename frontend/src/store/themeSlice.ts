// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { ThemePreference } from '@jalapenolabs/uikit'
import type { ResolvedTheme } from '../theme/themePreference'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

// Misc
import { readStoredThemePreference, resolveTheme } from '../theme/themePreference'

type ThemeState = {
  preference: ThemePreference
  // What the preference resolves to right now, so components need not query the OS.
  resolved: ResolvedTheme
}

type ThemeChange = {
  preference: ThemePreference
  resolved: ResolvedTheme
}

const initialPreference = readStoredThemePreference()

const initialState: ThemeState = {
  preference: initialPreference,
  resolved: resolveTheme(initialPreference),
}

export const themeSlice = createSlice({
  name: 'theme',
  initialState,
  reducers: {
    // A theme was chosen, in this tab or (through the storage event) another one, or
    // the OS color scheme changed. `startThemeSync` saves and applies the result.
    // Resolving reads the OS setting, so it happens while preparing the action and
    // the reducer stays pure.
    themeChanged: {
      reducer(state, action: PayloadAction<ThemeChange>) {
        state.preference = action.payload.preference
        state.resolved = action.payload.resolved
      },
      prepare(preference: ThemePreference) {
        return {
          payload: {
            preference,
            resolved: resolveTheme(preference),
          },
        }
      },
    },
  },
})

export const { themeChanged } = themeSlice.actions

export function selectThemePreference(state: RootState) {
  return state.theme.preference
}

export function selectResolvedTheme(state: RootState) {
  return state.theme.resolved
}
