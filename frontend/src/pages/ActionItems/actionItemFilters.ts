// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemState } from '../../api/routes/actionItemRoutes'

// Misc
import { ACTION_ITEM_STATES } from '../../api/routes/actionItemRoutes'
import { isSnoozed } from './actionItemPresentation'

// The All items list's filters, kept in the address so a filtered list survives opening an
// item and coming back, and can be linked to. The semantics mirror the API's list query.

// Any, only those that are, or only those that are not.
export const TRISTATE_CHOICES = [ 'any', 'yes', 'no' ] as const
export type Tristate = typeof TRISTATE_CHOICES[number]

// `project` filters on items in no project.
export const NO_PROJECT = 'none'

export type ActionItemFilters = {
  // Empty shows every state.
  states: ActionItemState[]
  // A project id, `NO_PROJECT`, or null for any.
  project: string | null
  // An initiative id, or null for any.
  initiative: string | null
  waiting: Tristate
  snoozed: Tristate
  // Deleted items only, instead of the rest.
  deleted: boolean
}

// With no filters in the address, the list shows what is still owed: the inbox and open
// items.
export const DEFAULT_STATES: ActionItemState[] = [ 'inbox', 'open' ]

const STATE_PARAM = 'state'
const PROJECT_PARAM = 'project'
const INITIATIVE_PARAM = 'initiative'
const WAITING_PARAM = 'waiting'
const SNOOZED_PARAM = 'snoozed'
const DELETED_PARAM = 'deleted'

// `state=all` shows every state; an absent `state` shows the defaults.
const ALL_STATES = 'all'

function readTristate(value: string | null): Tristate {
  if (value === 'yes' || value === 'no') {
    return value
  }
  return 'any'
}

export function readActionItemFilters(params: URLSearchParams): ActionItemFilters {
  const stateParam = params.get(STATE_PARAM)
  const states: ActionItemState[] = []
  if (stateParam === null) {
    states.push(...DEFAULT_STATES)
  }
  else if (stateParam !== ALL_STATES) {
    for (const state of ACTION_ITEM_STATES) {
      if (stateParam.split(',').includes(state)) {
        states.push(state)
      }
    }
  }

  return {
    states,
    project: params.get(PROJECT_PARAM),
    initiative: params.get(INITIATIVE_PARAM),
    waiting: readTristate(params.get(WAITING_PARAM)),
    snoozed: readTristate(params.get(SNOOZED_PARAM)),
    deleted: params.get(DELETED_PARAM) === 'true',
  }
}

// Only what differs from the defaults goes in the address, so the plain list has a plain
// address.
export function writeActionItemFilters(filters: ActionItemFilters) {
  const params = new URLSearchParams()

  const isDefaultStates = filters.states.length === DEFAULT_STATES.length
    && DEFAULT_STATES.every((state) => filters.states.includes(state))
  if (!isDefaultStates) {
    const stateValue = filters.states.length
      ? filters.states.join(',')
      : ALL_STATES
    params.set(STATE_PARAM, stateValue)
  }
  if (filters.project) {
    params.set(PROJECT_PARAM, filters.project)
  }
  if (filters.initiative) {
    params.set(INITIATIVE_PARAM, filters.initiative)
  }
  if (filters.waiting !== 'any') {
    params.set(WAITING_PARAM, filters.waiting)
  }
  if (filters.snoozed !== 'any') {
    params.set(SNOOZED_PARAM, filters.snoozed)
  }
  if (filters.deleted) {
    params.set(DELETED_PARAM, 'true')
  }
  return params
}

function matchesTristate(choice: Tristate, value: boolean) {
  if (choice === 'any') {
    return true
  }
  return (choice === 'yes') === value
}

// The items that pass every filter, in the order given. `items` are either the live or
// the deleted ones, as `filters.deleted` chose; this does not re-check deletion.
export function filterActionItems(items: ActionItem[], filters: ActionItemFilters, now: number) {
  const matches: ActionItem[] = []
  for (const item of items) {
    if (filters.states.length && !filters.states.includes(item.state)) {
      continue
    }
    if (filters.project === NO_PROJECT && item.projectIds.length) {
      continue
    }
    if (filters.project && filters.project !== NO_PROJECT && !item.projectIds.includes(filters.project)) {
      continue
    }
    if (filters.initiative && !item.initiativeIds.includes(filters.initiative)) {
      continue
    }
    if (!matchesTristate(filters.waiting, Boolean(item.waitingOn))) {
      continue
    }
    if (!matchesTristate(filters.snoozed, isSnoozed(item, now))) {
      continue
    }
    matches.push(item)
  }
  return matches
}
