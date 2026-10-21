// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { MeetingState } from '../../../api/routes/meetingRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export const meetingStateLabelKeys = {
  pending: 'states.pending',
  recorded: 'states.recorded',
  segmented: 'states.segmented',
  done: 'states.done',
  failed: 'states.failed',
} as const satisfies Record<MeetingState, ParseKeys<'meetings'>>

// Still working reads as accent, finished as success, given up as danger.
export const meetingStateChipColors = {
  pending: 'accent',
  recorded: 'accent',
  segmented: 'accent',
  done: 'success',
  failed: 'danger',
} as const satisfies Record<MeetingState, ChipColor>

// A meeting's length as a player shows it: `m:ss`, or `h:mm:ss` from an hour. Null for a
// meeting whose source did not say.
export function formatDuration(durationMs: number | null): string | null {
  if (durationMs === null || durationMs < 0) {
    return null
  }
  const totalSeconds = Math.floor(durationMs / 1000)
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  const paddedSeconds = String(seconds).padStart(2, '0')
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${paddedSeconds}`
  }
  return `${minutes}:${paddedSeconds}`
}
