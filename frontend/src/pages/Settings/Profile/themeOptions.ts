// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { ThemePreference } from '@jalapenolabs/uikit'
import type { SVGProps, JSX } from 'react'

// User interface
import { ThemePreviewDark, ThemePreviewLight, ThemePreviewSystem } from '@jalapenolabs/uikit'

type ThemeOption = {
  preference: ThemePreference
  labelKey: ParseKeys<'settings'>
  Preview: (props: SVGProps<SVGSVGElement>) => JSX.Element
}

// Display order of the theme cards. The artwork comes from @jalapenolabs/uikit.
export const themeOptions = [
  {
    preference: 'light',
    labelKey: 'personalDetails.appearance.options.light',
    Preview: ThemePreviewLight,
  },
  {
    preference: 'dark',
    labelKey: 'personalDetails.appearance.options.dark',
    Preview: ThemePreviewDark,
  },
  {
    preference: 'system',
    labelKey: 'personalDetails.appearance.options.system',
    Preview: ThemePreviewSystem,
  },
] as const satisfies readonly ThemeOption[]
