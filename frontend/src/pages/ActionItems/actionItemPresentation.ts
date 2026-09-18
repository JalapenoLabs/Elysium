// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  ActionItem,
  ActionItemPriority,
  ActionItemState,
  ActionItemTransition,
} from '../../api/routes/actionItemRoutes'

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export const stateLabelKeys = {
  inbox: 'states.inbox',
  open: 'states.open',
  resolved: 'states.resolved',
  dismissed: 'states.dismissed',
} as const satisfies Record<ActionItemState, ParseKeys<'actionItems'>>

export const stateChipColors = {
  inbox: 'accent',
  open: 'default',
  resolved: 'success',
  dismissed: 'default',
} as const satisfies Record<ActionItemState, ChipColor>

export const priorityLabelKeys = {
  urgent: 'priorities.urgent',
  high: 'priorities.high',
  normal: 'priorities.normal',
  low: 'priorities.low',
} as const satisfies Record<ActionItemPriority, ParseKeys<'actionItems'>>

export const priorityChipColors = {
  urgent: 'danger',
  high: 'warning',
  normal: 'default',
  low: 'default',
} as const satisfies Record<ActionItemPriority, ChipColor>

export const transitionLabelKeys = {
  accept: 'transitions.accept',
  resolve: 'transitions.resolve',
  dismiss: 'transitions.dismiss',
  reopen: 'transitions.reopen',
} as const satisfies Record<ActionItemTransition, ParseKeys<'actionItems'>>

// What each state allows, mirroring `Transition` in api/src/action_items/mod.rs, so a page
// offers only what the API would accept.
export const transitionsByState = {
  inbox: [ 'accept', 'resolve', 'dismiss' ],
  open: [ 'resolve', 'dismiss' ],
  resolved: [ 'reopen' ],
  dismissed: [ 'reopen' ],
} as const satisfies Record<ActionItemState, readonly ActionItemTransition[]>

// Actor kinds by the prefix before the colon, in history entries and comment authors.
const actorKeysByKind = new Map<string, ParseKeys<'actionItems'>>([
  [ 'user', 'actors.user' ],
  [ 'elysia', 'actors.elysia' ],
  [ 'session', 'actors.session' ],
  [ 'watcher', 'actors.watcher' ],
])

// Who did something: `user`, `elysia`, `session:<number>`, or `watcher:<provider>`, with
// the part after the colon as `detail`. An actor this build does not know is shown as sent.
export function describeActor(actor: string) {
  const [ kind, detail = '' ] = actor.split(':', 2)
  const key = actorKeysByKind.get(kind)
  if (!key) {
    console.debug('describeActor received an actor this build does not know', { actor })
    return { key: 'actors.unknown', values: { detail: actor }} as const
  }
  return { key, values: { detail }} as const
}

// The owner in words, for items that are not the user's.
export function describeOwner(item: ActionItem) {
  const owner = item.owner
  if (owner.kind === 'other') {
    return { key: 'owner.other', values: { name: owner.name }} as const
  }
  if (owner.kind === 'nobody') {
    return { key: 'owner.nobody', values: {}} as const
  }
  return { key: 'owner.user', values: {}} as const
}

// Overdue once the due moment has passed; an item due exactly now is not yet overdue, as
// Next decides.
export function isOverdue(item: ActionItem, now: number) {
  if (!item.dueAt) {
    return false
  }
  return Date.parse(item.dueAt) < now
}

// Whether a snooze still holds at `now`.
export function isSnoozed(item: ActionItem, now: number) {
  if (!item.snoozedUntil) {
    return false
  }
  return Date.parse(item.snoozedUntil) > now
}
