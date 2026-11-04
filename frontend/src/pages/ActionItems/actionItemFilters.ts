// Copyright © 2026 Jalapeno Labs

import type { SmartTableFilterValue, SmartTableFilterValues } from '@jalapenolabs/uikit'
import type { ActionItem, ActionItemState } from '../../api/routes/actionItemRoutes'

// Misc
import { ACTION_ITEM_STATES } from '../../api/routes/actionItemRoutes'
import { isSnoozed } from './actionItemPresentation'

// The All items list's filters, kept in the address so a filtered list survives opening an
// item and coming back, and can be linked to. The semantics mirror the API's list query.
// They are edited in the table's filters drawer and shown as its tags (uikit's `filters`),
// which speak `SmartTableFilterValues`; `toActionItemFilterValues` and
// `fromActionItemFilterValues` translate. Elysium applies them itself (`filterActionItems`),
// so the page knows when nothing matches and can say so.

// Any, only those that are, or only those that are not.
export type Tristate = 'any' | 'yes' | 'no'

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
  // Deleted items only, instead of the rest. In the drawer this is Yes; Any and No both list
  // the live items, the plain list.
  deleted: boolean
}

// With no filters in the address, the list shows what is still owed: the inbox and open
// items.
export const DEFAULT_STATES: ActionItemState[] = [ 'inbox', 'open' ]

// Each filter's id in the drawer, which is also its parameter in the address.
export const ACTION_ITEM_FILTER_IDS = {
  states: 'state',
  project: 'project',
  initiative: 'initiative',
  waiting: 'waiting',
  snoozed: 'snoozed',
  deleted: 'deleted',
} as const satisfies Record<keyof ActionItemFilters, string>

const STATE_PARAM = ACTION_ITEM_FILTER_IDS.states
const PROJECT_PARAM = ACTION_ITEM_FILTER_IDS.project
const INITIATIVE_PARAM = ACTION_ITEM_FILTER_IDS.initiative
const WAITING_PARAM = ACTION_ITEM_FILTER_IDS.waiting
const SNOOZED_PARAM = ACTION_ITEM_FILTER_IDS.snoozed
const DELETED_PARAM = ACTION_ITEM_FILTER_IDS.deleted

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

// A yes or no filter in the drawer: unset is either.
function tristateToValue(choice: Tristate) {
  if (choice === 'any') {
    return null
  }
  return choice === 'yes'
}

function tristateFromValue(value: SmartTableFilterValue | undefined): Tristate {
  if (typeof value !== 'boolean') {
    return 'any'
  }
  if (value) {
    return 'yes'
  }
  return 'no'
}

function idFromValue(value: SmartTableFilterValue | undefined) {
  if (typeof value === 'string' && value) {
    return value
  }
  return null
}

// The filters as the drawer and its tags show them. The default states are a filter like any
// other, so their tag says what the plain list leaves out; every state is no filter at all.
// Showing live items is the plain list too, so only showing deleted ones has a tag.
export function toActionItemFilterValues(filters: ActionItemFilters): SmartTableFilterValues {
  return {
    [ACTION_ITEM_FILTER_IDS.states]: filters.states.length
      ? filters.states
      : null,
    [ACTION_ITEM_FILTER_IDS.project]: filters.project,
    [ACTION_ITEM_FILTER_IDS.initiative]: filters.initiative,
    [ACTION_ITEM_FILTER_IDS.waiting]: tristateToValue(filters.waiting),
    [ACTION_ITEM_FILTER_IDS.snoozed]: tristateToValue(filters.snoozed),
    [ACTION_ITEM_FILTER_IDS.deleted]: filters.deleted || null,
  }
}

// The filters the drawer applied, or a tag removed. Values come from the drawer, which only
// offers the states it was given, but are checked like the address is.
export function fromActionItemFilterValues(values: SmartTableFilterValues): ActionItemFilters {
  const stateValue = values[ACTION_ITEM_FILTER_IDS.states]
  const states: ActionItemState[] = []
  if (Array.isArray(stateValue)) {
    for (const state of ACTION_ITEM_STATES) {
      if (stateValue.includes(state)) {
        states.push(state)
      }
    }
  }

  return {
    states,
    project: idFromValue(values[ACTION_ITEM_FILTER_IDS.project]),
    initiative: idFromValue(values[ACTION_ITEM_FILTER_IDS.initiative]),
    waiting: tristateFromValue(values[ACTION_ITEM_FILTER_IDS.waiting]),
    snoozed: tristateFromValue(values[ACTION_ITEM_FILTER_IDS.snoozed]),
    deleted: values[ACTION_ITEM_FILTER_IDS.deleted] === true,
  }
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
