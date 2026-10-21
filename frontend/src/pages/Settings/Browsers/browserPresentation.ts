// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { BrowserState } from '../../../api/routes/browserSessionRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export const stateLabelKeys = {
  starting: 'states.starting',
  running: 'states.running',
  failed: 'states.failed',
} as const satisfies Record<BrowserState, ParseKeys<'browsers'>>

export const stateChipColors = {
  starting: 'warning',
  running: 'success',
  failed: 'danger',
} as const satisfies Record<BrowserState, ChipColor>
