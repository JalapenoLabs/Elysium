// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { CodingSession, ThreadState, TurnState } from '../../api/routes/codingSessionRoutes'

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

export type SessionTranslate = TFunction<'coding'>

export type SessionContext = {
  projectNames: Record<string, string>
  satelliteNames: Record<string, string>
  // Renders a UTC instant in the viewer's locale and time zone.
  formatInstant: (instant: string) => string
}

// What a list of sessions shows for one session. The Sessions panel's table, its tiles,
// and its search all read this, so the two views can never word a session differently.
export type SessionSummary = {
  session: CodingSession
  projectName: string
  satelliteName: string
  state: ThreadState
  stateLabel: string
  lastActivity: string
}

export function describeSession(
  session: CodingSession,
  t: SessionTranslate,
  context: SessionContext,
): SessionSummary {
  // The thread is null until the API's first poll sees it.
  const state = session.thread?.state ?? 'unknown'
  const lastActivityAt = session.thread?.lastActivityAt
  const lastActivity = lastActivityAt
    ? context.formatInstant(lastActivityAt)
    : t('sessions.never')

  return {
    session,
    projectName: context.projectNames[session.projectId] ?? '',
    satelliteName: context.satelliteNames[session.satelliteId] ?? '',
    state,
    stateLabel: t(threadStateLabelKeys[state]),
    lastActivity,
  }
}

// The summaries whose number, title, project, satellite, state, or last activity contains
// `search`, case-insensitively and in their given order: what either view shows, as the
// viewer reads it.
export function searchSessionSummaries(summaries: SessionSummary[], search: string): SessionSummary[] {
  const needle = search.trim().toLocaleLowerCase()
  if (!needle) {
    return summaries
  }

  return summaries.filter((summary) => {
    const haystack = [
      String(summary.session.id),
      summary.session.title,
      summary.projectName,
      summary.satelliteName,
      summary.stateLabel,
      summary.lastActivity,
    ].join('\n')
    return haystack.toLocaleLowerCase().includes(needle)
  })
}
