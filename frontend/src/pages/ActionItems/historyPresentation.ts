// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { ActionItemOwner, HistoryEntry, HistoryKind } from '../../api/routes/actionItemRoutes'

// Misc
import { ACTION_ITEM_PRIORITIES, ACTION_ITEM_STATES, HISTORY_KINDS } from '../../api/routes/actionItemRoutes'
import { INITIATIVE_STATES } from '../../api/routes/initiativeRoutes'
import { initiativeStateLabelKeys } from '../Initiatives/initiativePresentation'
import { priorityLabelKeys, stateLabelKeys } from './actionItemPresentation'

// Turns history entries into sentences. `data` arrives as JSON whose shape follows the
// entry's kind (docs/action-items.md), so every field is read defensively: an entry from a
// newer API, or one missing a field, still reads as something rather than breaking the page.

export type HistoryTranslate = TFunction<[ 'actionItems', 'initiatives' ]>

export type HistoryContext = {
  // Whose history this is: the sentence for an item joining an initiative names the other
  // side.
  subject: 'item' | 'initiative'
  projectNames: Record<string, string>
  initiativeNames: Record<string, string>
  itemTitles: Record<string, string>
  // Formats an instant for the viewer.
  formatInstant: (instant: string) => string
}

export type HistoryDescription = {
  summary: string
  // Lines under the summary: each field an edit changed, or a comment's text.
  details: string[]
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function readString(data: unknown, key: string) {
  if (!isRecord(data)) {
    return null
  }
  const value = data[key]
  if (typeof value === 'string') {
    return value
  }
  return null
}

function isOwner(value: unknown): value is ActionItemOwner {
  return isRecord(value) && typeof value.kind === 'string'
}

function isOneOf<Value extends string>(options: readonly Value[], value: unknown): value is Value {
  return typeof value === 'string' && options.some((option) => option === value)
}

// A name from a lookup, or the fallback for an id it lacks, such as a deleted project's.
function lookupName(names: Record<string, string>, id: string | null, fallback: string) {
  if (!id) {
    return fallback
  }
  return names[id] ?? fallback
}

// Item and initiative fields an edit can change. A field a newer API adds is named as sent.
const fieldLabelKeys = new Map<string, ParseKeys<'actionItems'>>([
  [ 'title', 'history.fields.title' ],
  [ 'notes', 'history.fields.notes' ],
  [ 'priority', 'history.fields.priority' ],
  [ 'dueAt', 'history.fields.dueAt' ],
  [ 'snoozedUntil', 'history.fields.snoozedUntil' ],
  [ 'waitingOn', 'history.fields.waitingOn' ],
  [ 'owner', 'history.fields.owner' ],
  [ 'name', 'history.fields.name' ],
  [ 'description', 'history.fields.description' ],
  [ 'targetAt', 'history.fields.targetAt' ],
  [ 'state', 'history.fields.state' ],
])

// Long text is named as changed rather than quoted.
const LONG_TEXT_FIELDS = new Set([ 'notes', 'description' ])
const INSTANT_FIELDS = new Set([ 'dueAt', 'snoozedUntil', 'targetAt' ])

// One side of an edit, in words.
function describeValue(field: string, value: unknown, context: HistoryContext, t: HistoryTranslate) {
  if (value === null || value === undefined || value === '') {
    return t('history.values.none')
  }
  if (INSTANT_FIELDS.has(field) && typeof value === 'string') {
    return context.formatInstant(value)
  }
  if (field === 'priority' && isOneOf(ACTION_ITEM_PRIORITIES, value)) {
    return t(priorityLabelKeys[value])
  }
  if (field === 'state' && isOneOf(INITIATIVE_STATES, value)) {
    return t(initiativeStateLabelKeys[value], { ns: 'initiatives' })
  }
  if (field === 'owner' && isOwner(value)) {
    if (value.kind === 'other') {
      return value.name
    }
    if (value.kind === 'nobody') {
      return t('owner.nobody')
    }
    return t('owner.user')
  }
  if (typeof value === 'string') {
    return value
  }
  return JSON.stringify(value)
}

function describeChanges(data: unknown, context: HistoryContext, t: HistoryTranslate) {
  const changes = isRecord(data) && isRecord(data.changes)
    ? data.changes
    : {}
  const details: string[] = []

  for (const [ field, change ] of Object.entries(changes)) {
    const labelKey = fieldLabelKeys.get(field)
    const label = labelKey
      ? t(labelKey)
      : field
    if (LONG_TEXT_FIELDS.has(field) || !isRecord(change)) {
      details.push(t('history.fieldChanged', { field: label }))
      continue
    }
    details.push(t('history.fieldChangedFromTo', {
      field: label,
      from: describeValue(field, change.from, context, t),
      to: describeValue(field, change.to, context, t),
    }))
  }
  return details
}

function describeStateChange(data: unknown, t: HistoryTranslate) {
  const from = isRecord(data)
    ? data.from
    : null
  const to = isRecord(data)
    ? data.to
    : null
  if (!isOneOf(ACTION_ITEM_STATES, from) || !isOneOf(ACTION_ITEM_STATES, to)) {
    console.debug('A state change entry is missing its states', { data })
    return t('history.stateChangedUnknown')
  }
  return t('history.stateChanged', { from: t(stateLabelKeys[from]), to: t(stateLabelKeys[to]) })
}

type Describe = (entry: HistoryEntry, context: HistoryContext, t: HistoryTranslate) => HistoryDescription

// One sentence per kind, and the details worth showing under it.
const describeByKind = {
  created: (_entry, context, t) => ({
    summary: context.subject === 'item'
      ? t('history.createdItem')
      : t('history.createdInitiative'),
    details: [],
  }),
  updated: (entry, context, t) => ({
    summary: t('history.updated'),
    details: describeChanges(entry.data, context, t),
  }),
  state_changed: (entry, _context, t) => ({
    summary: describeStateChange(entry.data, t),
    details: [],
  }),
  deleted: (_entry, _context, t) => ({
    summary: t('history.deleted'),
    details: [],
  }),
  restored: (_entry, _context, t) => ({
    summary: t('history.restored'),
    details: [],
  }),
  commented: (entry, _context, t) => ({
    summary: t('history.commented'),
    details: [ readString(entry.data, 'body') ?? '' ].filter(Boolean),
  }),
  comment_edited: (_entry, _context, t) => ({
    summary: t('history.commentEdited'),
    details: [],
  }),
  comment_deleted: (entry, _context, t) => ({
    summary: t('history.commentDeleted'),
    details: [ readString(entry.data, 'body') ?? '' ].filter(Boolean),
  }),
  project_added: (entry, context, t) => ({
    summary: t('history.projectAdded', {
      name: lookupName(
        context.projectNames,
        readString(entry.data, 'projectId'),
        t('history.values.deletedProject'),
      ),
    }),
    details: [],
  }),
  project_removed: (entry, context, t) => ({
    summary: t('history.projectRemoved', {
      name: lookupName(
        context.projectNames,
        readString(entry.data, 'projectId'),
        t('history.values.deletedProject'),
      ),
    }),
    details: [],
  }),
  initiative_joined: (entry, context, t) => ({
    summary: context.subject === 'item'
      ? t('history.joinedInitiative', {
        name: lookupName(context.initiativeNames, entry.initiativeId, t('history.values.unknownInitiative')),
      })
      : t('history.itemJoined', {
        title: lookupName(context.itemTitles, entry.actionItemId, t('history.values.unknownItem')),
      }),
    details: [],
  }),
  initiative_left: (entry, context, t) => ({
    summary: context.subject === 'item'
      ? t('history.leftInitiative', {
        name: lookupName(context.initiativeNames, entry.initiativeId, t('history.values.unknownInitiative')),
      })
      : t('history.itemLeft', {
        title: lookupName(context.itemTitles, entry.actionItemId, t('history.values.unknownItem')),
      }),
    details: [],
  }),
} as const satisfies Record<HistoryKind, Describe>

export function describeHistoryEntry(
  entry: HistoryEntry,
  context: HistoryContext,
  t: HistoryTranslate,
): HistoryDescription {
  if (!isOneOf(HISTORY_KINDS, entry.kind)) {
    console.debug('A history entry has a kind this build does not know', { kind: entry.kind })
    return { summary: t('history.unknownKind', { kind: entry.kind }), details: []}
  }
  return describeByKind[entry.kind](entry, context, t)
}
