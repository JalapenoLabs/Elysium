// Copyright © 2026 Jalapeno Labs

import type { ThemePreference } from '@jalapenolabs/uikit'

// Core
import { useCallback } from 'react'

// Redux
import { useAppDispatch, useAppSelector } from '../store/hooks'
import { selectThemePreference, themeChanged } from '../store/themeSlice'

export function useThemePreference() {
  const dispatch = useAppDispatch()
  const preference = useAppSelector(selectThemePreference)

  const setPreference = useCallback(
    (next: ThemePreference) => dispatch(themeChanged(next)),
    [ dispatch ],
  )

  return {
    preference,
    setPreference,
  } as const
}
