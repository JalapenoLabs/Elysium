// Copyright © 2026 Jalapeno Labs

import type { SmartTableFilterValues } from '@jalapenolabs/uikit'
import type { Initiative, InitiativeState } from '../../api/routes/initiativeRoutes'

// Misc
import { INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { NO_PROJECT } from '../ActionItems/actionItemFilters'

// The Initiatives list's filters, kept in the address the way the item list keeps its own,
// edited in the table's filters drawer and shown as its tags. Elysium applies them itself
// (`filterInitiatives`), so the page knows when nothing matches.

export type InitiativeFilters = {
  // Empty shows every state.
  states: InitiativeState[]
  // A project id, `NO_PROJECT`, or null for any.
  project: string | null
  // Deleted initiatives only, instead of the rest. In the drawer this is Yes; Any and No both
  // list the live ones, the plain list.
  deleted: boolean
}

// With no filters in the address, the list shows the goals still being worked toward.
export const DEFAULT_INITIATIVE_STATES: InitiativeState[] = [ 'active' ]

// Each filter's id in the drawer, which is also its parameter in the address.
export const INITIATIVE_FILTER_IDS = {
  states: 'state',
  project: 'project',
  deleted: 'deleted',
} as const satisfies Record<keyof InitiativeFilters, string>

// `state=all` shows every state; an absent `state` shows the defaults.
const ALL_STATES = 'all'

function readStates(stateList: string[]) {
  return INITIATIVE_STATES.filter((state) => stateList.includes(state))
}

export function readInitiativeFilters(params: URLSearchParams): InitiativeFilters {
  const stateParam = params.get(INITIATIVE_FILTER_IDS.states)
  let states: InitiativeState[] = []
  if (stateParam === null) {
    states = [ ...DEFAULT_INITIATIVE_STATES ]
  }
  else if (stateParam !== ALL_STATES) {
    states = readStates(stateParam.split(','))
  }

  return {
    states,
    project: params.get(INITIATIVE_FILTER_IDS.project),
    deleted: params.get(INITIATIVE_FILTER_IDS.deleted) === 'true',
  }
}

// Only what differs from the defaults goes in the address, so the plain list has a plain
// address.
export function writeInitiativeFilters(filters: InitiativeFilters) {
  const params = new URLSearchParams()

  const isDefaultStates = filters.states.length === DEFAULT_INITIATIVE_STATES.length
    && DEFAULT_INITIATIVE_STATES.every((state) => filters.states.includes(state))
  if (!isDefaultStates) {
    const stateValue = filters.states.length
      ? filters.states.join(',')
      : ALL_STATES
    params.set(INITIATIVE_FILTER_IDS.states, stateValue)
  }
  if (filters.project) {
    params.set(INITIATIVE_FILTER_IDS.project, filters.project)
  }
  if (filters.deleted) {
    params.set(INITIATIVE_FILTER_IDS.deleted, 'true')
  }
  return params
}

// The filters as the drawer and its tags show them. The default state is a filter like any
// other, so its tag says what the plain list leaves out.
export function toInitiativeFilterValues(filters: InitiativeFilters): SmartTableFilterValues {
  return {
    [INITIATIVE_FILTER_IDS.states]: filters.states.length
      ? filters.states
      : null,
    [INITIATIVE_FILTER_IDS.project]: filters.project,
    [INITIATIVE_FILTER_IDS.deleted]: filters.deleted || null,
  }
}

// The filters the drawer applied, or a tag removed.
export function fromInitiativeFilterValues(values: SmartTableFilterValues): InitiativeFilters {
  const stateValue = values[INITIATIVE_FILTER_IDS.states]
  const projectValue = values[INITIATIVE_FILTER_IDS.project]

  return {
    states: Array.isArray(stateValue)
      ? readStates(stateValue)
      : [],
    project: typeof projectValue === 'string' && projectValue
      ? projectValue
      : null,
    deleted: values[INITIATIVE_FILTER_IDS.deleted] === true,
  }
}

// The initiatives that pass every filter, in the order given. `initiatives` are either the
// live or the deleted ones, as `filters.deleted` chose; this does not re-check deletion.
export function filterInitiatives(initiatives: Initiative[], filters: InitiativeFilters) {
  const matches: Initiative[] = []
  for (const initiative of initiatives) {
    if (filters.states.length && !filters.states.includes(initiative.state)) {
      continue
    }
    if (filters.project === NO_PROJECT && initiative.projectIds.length) {
      continue
    }
    if (filters.project && filters.project !== NO_PROJECT && !initiative.projectIds.includes(filters.project)) {
      continue
    }
    matches.push(initiative)
  }
  return matches
}
