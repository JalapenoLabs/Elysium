// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { InitiativeState } from '../../api/routes/initiativeRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export const initiativeStateLabelKeys = {
  active: 'states.active',
  achieved: 'states.achieved',
  abandoned: 'states.abandoned',
} as const satisfies Record<InitiativeState, ParseKeys<'initiatives'>>

export const initiativeStateChipColors = {
  active: 'accent',
  achieved: 'success',
  abandoned: 'default',
} as const satisfies Record<InitiativeState, ChipColor>

// An initiative's name and description, bounded as the API bounds them.
export const INITIATIVE_NAME_MAX_CHARACTERS = 200
export const INITIATIVE_DESCRIPTION_MAX_CHARACTERS = 20_000
