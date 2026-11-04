// Copyright © 2026 Jalapeno Labs

import type { SmartTableFilterValues } from '@jalapenolabs/uikit'
import type { ActivityState } from '../../api/routes/codingSessionRoutes'
import type { SessionSummary } from './sessionPresentation'

// Misc
import { ACTIVITY_STATES } from '../../api/routes/codingSessionRoutes'

// The Sessions panel's filters, kept in the Coding page's address the way the action item
// list keeps its own, edited in the table's filters drawer and shown as its tags. Elysium
// applies them itself (`filterSessionSummaries`), so the table and the tiles list the same
// sessions, and the panel knows when nothing matches.

export type SessionFilters = {
  // Empty shows every state.
  states: ActivityState[]
  // A project id, or null for any.
  project: string | null
}

// Each filter's id in the drawer, which is also its parameter in the address.
export const SESSION_FILTER_IDS = {
  states: 'state',
  project: 'project',
} as const satisfies Record<keyof SessionFilters, string>

function readStates(stateList: readonly string[]) {
  return ACTIVITY_STATES.filter((state) => stateList.includes(state))
}

export function readSessionFilters(params: URLSearchParams): SessionFilters {
  return {
    states: readStates(params.get(SESSION_FILTER_IDS.states)?.split(',') ?? []),
    project: params.get(SESSION_FILTER_IDS.project) || null,
  }
}

// Writes the filters over `params`, keeping whatever else the page holds there.
export function writeSessionFilters(params: URLSearchParams, filters: SessionFilters) {
  const next = new URLSearchParams(params)
  next.delete(SESSION_FILTER_IDS.states)
  next.delete(SESSION_FILTER_IDS.project)
  if (filters.states.length) {
    next.set(SESSION_FILTER_IDS.states, filters.states.join(','))
  }
  if (filters.project) {
    next.set(SESSION_FILTER_IDS.project, filters.project)
  }
  return next
}

export function toSessionFilterValues(filters: SessionFilters): SmartTableFilterValues {
  return {
    [SESSION_FILTER_IDS.states]: filters.states.length
      ? filters.states
      : null,
    [SESSION_FILTER_IDS.project]: filters.project,
  }
}

export function fromSessionFilterValues(values: SmartTableFilterValues): SessionFilters {
  const stateValue = values[SESSION_FILTER_IDS.states]
  const projectValue = values[SESSION_FILTER_IDS.project]

  return {
    states: Array.isArray(stateValue)
      ? readStates(stateValue)
      : [],
    project: typeof projectValue === 'string' && projectValue
      ? projectValue
      : null,
  }
}

// The summaries that pass every filter, in the order given.
export function filterSessionSummaries(summaries: SessionSummary[], filters: SessionFilters) {
  const matches: SessionSummary[] = []
  for (const summary of summaries) {
    if (filters.states.length && !filters.states.includes(summary.state)) {
      continue
    }
    if (filters.project && summary.session.projectId !== filters.project) {
      continue
    }
    matches.push(summary)
  }
  return matches
}
