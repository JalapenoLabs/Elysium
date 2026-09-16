// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { ThreadState, TurnState } from '../../api/routes/codingSessionRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export const threadStateLabelKeys = {
  'unknown': 'threadStates.unknown',
  'provisioning': 'threadStates.provisioning',
  'idle': 'threadStates.idle',
  'running': 'threadStates.running',
  'awaiting-input': 'threadStates.awaiting-input',
  'watching': 'threadStates.watching',
  'paused': 'threadStates.paused',
  'expired': 'threadStates.expired',
  'destroyed': 'threadStates.destroyed',
} as const satisfies Record<ThreadState, ParseKeys<'coding'>>

export const threadStateChipColors = {
  'unknown': 'default',
  'provisioning': 'default',
  'idle': 'success',
  'running': 'accent',
  'awaiting-input': 'warning',
  'watching': 'accent',
  'paused': 'default',
  'expired': 'danger',
  'destroyed': 'danger',
} as const satisfies Record<ThreadState, ChipColor>

// The session number column, in pixels: wide enough for four digits and the sort icon.
// Set in full because uikit's SmartTable otherwise holds every column to at least 160.
export const SESSION_NUMBER_COLUMN_SIZING = {
  size: 80,
  minSize: 64,
  maxSize: 120,
} as const

// A thread in one of these states can no longer take prompts.
export const CLOSED_THREAD_STATES: readonly ThreadState[] = [ 'expired', 'destroyed' ]

export const turnStateLabelKeys = {
  unknown: 'turnStates.unknown',
  queued: 'turnStates.queued',
  running: 'turnStates.running',
  completed: 'turnStates.completed',
  failed: 'turnStates.failed',
  cancelled: 'turnStates.cancelled',
  interrupted: 'turnStates.interrupted',
  watching: 'turnStates.watching',
} as const satisfies Record<TurnState, ParseKeys<'coding'>>
