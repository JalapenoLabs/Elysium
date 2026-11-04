// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { RunState, StepAction, StepState } from '../../api/routes/runRoutes'

// Names, colors, durations, and the actions each step's state allows, for the Runs pages.
// Pure, and tested beside.

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const runStateLabelKeys = {
  queued: 'runStates.queued',
  running: 'runStates.running',
  held: 'runStates.held',
  failed: 'runStates.failed',
  succeeded: 'runStates.succeeded',
} as const satisfies Record<RunState, ParseKeys<'runs'>>

export const runStateChipColors = {
  queued: 'default',
  running: 'accent',
  held: 'warning',
  failed: 'danger',
  succeeded: 'success',
} as const satisfies Record<RunState, ChipColor>

export const stepStateLabelKeys = {
  pending: 'stepStates.pending',
  queued: 'stepStates.queued',
  running: 'stepStates.running',
  held: 'stepStates.held',
  failed: 'stepStates.failed',
  skipped: 'stepStates.skipped',
  succeeded: 'stepStates.succeeded',
} as const satisfies Record<StepState, ParseKeys<'runs'>>

// What a person may do to a step in each state, as the API allows it: a failed step is
// retried or skipped, a held one resumed or skipped, and one not yet run skipped ahead.
export const stepActionsByState = {
  pending: [ 'skip' ],
  queued: [ 'skip' ],
  running: [],
  held: [ 'resume', 'skip' ],
  failed: [ 'retry', 'skip' ],
  skipped: [],
  succeeded: [],
} as const satisfies Record<StepState, readonly StepAction[]>

// A workflow's, step's, or source's name in code, such as `fetch_transcript`, as words:
// "Fetch transcript". Workflows declare their names in code, so this is how a page names
// one it has no words of its own for.
export function nameFromIdentifier(identifier: string) {
  const words = identifier
    .split('_')
    .filter(Boolean)
    .join(' ')
  if (!words) {
    console.debug('nameFromIdentifier was given no name, showing it as it is', { identifier })
    return identifier
  }
  return words.charAt(0).toUpperCase() + words.slice(1)
}

type Duration = {
  key: ParseKeys<'runs'>
  values: Record<string, number>
}

// How long something took from `startedAt` until `finishedAt`, or until `now` while it runs,
// to the second: "42s", "3m 4s", "2h 5m". `null` before it started.
export function durationBetween(
  startedAt: string | null,
  finishedAt: string | null,
  now: number,
): Duration | null {
  if (!startedAt) {
    return null
  }
  const end = finishedAt
    ? Date.parse(finishedAt)
    : now
  const totalSeconds = Math.max(0, Math.floor((end - Date.parse(startedAt)) / 1000))
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = totalSeconds % 60
  if (hours) {
    return { key: 'durations.hours', values: { hours, minutes }}
  }
  if (minutes) {
    return { key: 'durations.minutes', values: { minutes, seconds }}
  }
  return { key: 'durations.seconds', values: { seconds }}
}
