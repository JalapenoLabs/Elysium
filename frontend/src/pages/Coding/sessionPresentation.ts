// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { ActivityState, CodingSession, TurnState } from '../../api/routes/codingSessionRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export const activityStateLabelKeys = {
  idle: 'activityStates.idle',
  queued: 'activityStates.queued',
  running: 'activityStates.running',
} as const satisfies Record<ActivityState, ParseKeys<'coding'>>

export const activityStateChipColors = {
  idle: 'success',
  queued: 'default',
  running: 'accent',
} as const satisfies Record<ActivityState, ChipColor>

// The session number column, in pixels: wide enough for four digits and the sort icon.
// Set in full because uikit's SmartTable otherwise holds every column to at least 160.
export const SESSION_NUMBER_COLUMN_SIZING = {
  size: 80,
  minSize: 64,
  maxSize: 120,
} as const

export const turnStateLabelKeys = {
  'queued': 'turnStates.queued',
  'completed': 'turnStates.completed',
  'failed': 'turnStates.failed',
  'cancelled': 'turnStates.cancelled',
  'interrupted': 'turnStates.interrupted',
  'usage-limited': 'turnStates.usage-limited',
} as const satisfies Record<TurnState, ParseKeys<'coding'>>

export const turnStateChipColors = {
  'queued': 'default',
  'completed': 'success',
  'failed': 'danger',
  'cancelled': 'default',
  'interrupted': 'warning',
  'usage-limited': 'warning',
} as const satisfies Record<TurnState, ChipColor>

export type SessionTranslate = TFunction<'coding'>

export type SessionContext = {
  projectNames: Record<string, string>
  // Renders a UTC instant in the viewer's locale and time zone.
  formatInstant: (instant: string) => string
}

// What a list of sessions shows for one session. The Sessions panel's table, its tiles,
// and its search all read this, so the two views can never word a session differently.
export type SessionSummary = {
  session: CodingSession
  projectName: string
  state: ActivityState
  stateLabel: string
  lastActivity: string
}

export function describeSession(
  session: CodingSession,
  t: SessionTranslate,
  context: SessionContext,
): SessionSummary {
  const lastActivityAt = session.activity.lastActivityAt
  const lastActivity = lastActivityAt
    ? context.formatInstant(lastActivityAt)
    : t('sessions.never')

  return {
    session,
    projectName: session.projectId
      ? context.projectNames[session.projectId] ?? ''
      : '',
    state: session.activity.state,
    stateLabel: t(activityStateLabelKeys[session.activity.state]),
    lastActivity,
  }
}

// The summaries whose number, title, project, state, or last activity contains `search`,
// case-insensitively and in their given order: what either view shows, as the viewer
// reads it.
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
      summary.stateLabel,
      summary.lastActivity,
    ].join('\n')
    return haystack.toLocaleLowerCase().includes(needle)
  })
}
