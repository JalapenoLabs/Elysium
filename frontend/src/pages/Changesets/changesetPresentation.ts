// Copyright © 2026 Jalapeno Labs

import type { ParseKeys, TFunction } from 'i18next'
import type { ActionItem, LinkProvider } from '../../api/routes/actionItemRoutes'
import type {
  Changeset,
  ChangesetDecision,
  ChangesetOperation,
  ChangesetOperationChange,
  ChangesetOperationKind,
  ChangesetOutcome,
  ChangesetState,
  OperationTarget,
  OperationUndo,
} from '../../api/routes/changesetRoutes'

// Misc
import { ACTION_ITEM_PRIORITIES } from '../../api/routes/actionItemRoutes'
import { priorityLabelKeys } from '../ActionItems/actionItemPresentation'
import { providerLabelKeys } from '../ActionItems/linkPresentation'

// Names, colors, and sentences for changesets, and the dependency rule the API applies, so
// the review shows what a decision will do before it is made. Pure, and tested beside.

type ChipColor = 'accent' | 'success' | 'warning' | 'danger' | 'default'

export type ChangesetTranslate = TFunction<[ 'changesets', 'actionItems' ]>

export const changesetStateLabelKeys = {
  pending: 'states.pending',
  applied: 'states.applied',
  rejected: 'states.rejected',
  undone: 'states.undone',
} as const satisfies Record<ChangesetState, ParseKeys<'changesets'>>

export const changesetStateChipColors = {
  pending: 'accent',
  applied: 'success',
  rejected: 'default',
  undone: 'warning',
} as const satisfies Record<ChangesetState, ChipColor>

export const decisionLabelKeys = {
  pending: 'decisions.pending',
  approved: 'decisions.approved',
  rejected: 'decisions.rejected',
} as const satisfies Record<ChangesetDecision, ParseKeys<'changesets'>>

export const outcomeLabelKeys = {
  pending: 'outcomes.pending',
  applied: 'outcomes.applied',
  failed: 'outcomes.failed',
  skipped: 'outcomes.skipped',
  undone: 'outcomes.undone',
} as const satisfies Record<ChangesetOutcome, ParseKeys<'changesets'>>

export const outcomeChipColors = {
  pending: 'default',
  applied: 'success',
  failed: 'danger',
  skipped: 'default',
  undone: 'warning',
} as const satisfies Record<ChangesetOutcome, ChipColor>

export const operationKindLabelKeys = {
  'create-item': 'kinds.create-item',
  'update-item': 'kinds.update-item',
  'resolve-item': 'kinds.resolve-item',
  'dismiss-item': 'kinds.dismiss-item',
  'comment': 'kinds.comment',
  'link': 'kinds.link',
  'add-to-initiative': 'kinds.add-to-initiative',
  'remove-from-initiative': 'kinds.remove-from-initiative',
  'create-initiative': 'kinds.create-initiative',
} as const satisfies Record<ChangesetOperationKind, ParseKeys<'changesets'>>

// The fields an update can change, with their names as history spells them.
const updateFieldLabelKeys = {
  title: 'actionItems:history.fields.title',
  notes: 'actionItems:history.fields.notes',
  priority: 'actionItems:history.fields.priority',
  dueAt: 'actionItems:history.fields.dueAt',
} as const
const UPDATE_FIELDS = [
  'title',
  'notes',
  'priority',
  'dueAt',
] as const satisfies (keyof typeof updateFieldLabelKeys)[]
type UpdateField = typeof UPDATE_FIELDS[number]

function isUpdateField(field: string): field is UpdateField {
  return UPDATE_FIELDS.some((updateField) => updateField === field)
}

// Every operation that depends on the one at `position`, directly or through another, in
// order: what rejecting it also rejects. Mirrors `decide` in
// api/src/action_items/changesets.rs.
export function dependentsOf(operations: ChangesetOperation[], position: number) {
  const blocked = new Set([ position ])
  const dependents: number[] = []
  // Dependencies always come earlier, so one pass in order reaches every dependent.
  for (const operation of operations) {
    if (operation.dependsOn.some((dependency) => blocked.has(dependency))) {
      blocked.add(operation.position)
      dependents.push(operation.position)
    }
  }
  return dependents
}

// How many operations stand at each decision, and whether the changeset can be applied:
// pending, with every operation decided.
export function tallyDecisions(changeset: Changeset) {
  const tally = { approved: 0, rejected: 0, pending: 0 }
  for (const operation of changeset.operations) {
    tally[operation.decision] += 1
  }
  const canApply = changeset.state === 'pending' && tally.pending === 0
  return { ...tally, canApply } as const
}

export type DescribeContext = {
  operations: ChangesetOperation[]
  // Live items, by id, for names and the current values an edit replaces.
  itemsById: Record<string, ActionItem>
  initiativeNames: Record<string, string>
  projectNames: Record<string, string>
  formatInstant: (instant: string) => string
}

export type OperationDescription = {
  summary: string
  // Lines under the summary: each field an edit changes, a comment's text, where a new
  // item lands.
  details: string[]
}

// What the create at `position` will be called, if it names one.
function proposedName(context: DescribeContext, position: number) {
  const created = context.operations.find((operation) => operation.position === position)?.operation
  if (created?.kind === 'create-item') {
    return created.title
  }
  if (created?.kind === 'create-initiative') {
    return created.name
  }
  return null
}

// An item or initiative an operation names, in words.
function describeTarget(
  target: OperationTarget,
  subject: 'item' | 'initiative',
  context: DescribeContext,
  t: ChangesetTranslate,
) {
  if ('operation' in target) {
    const name = proposedName(context, target.operation)
    if (!name) {
      return t('describe.proposedUnnamed', { position: target.operation })
    }
    return t('describe.proposedRecord', { name, position: target.operation })
  }
  if (subject === 'item') {
    return context.itemsById[target.id]?.title ?? t('describe.missingItem')
  }
  return context.initiativeNames[target.id] ?? t('describe.missingInitiative')
}

// One value of an edited field, in words.
function describeFieldValue(
  field: UpdateField,
  value: unknown,
  context: DescribeContext,
  t: ChangesetTranslate,
) {
  if (value === null || value === undefined || value === '') {
    return t('describe.none')
  }
  if (field === 'dueAt' && typeof value === 'string') {
    return context.formatInstant(value)
  }
  const priority = ACTION_ITEM_PRIORITIES.find((option) => option === value)
  if (field === 'priority' && priority) {
    return t(priorityLabelKeys[priority], { ns: 'actionItems' })
  }
  return String(value)
}

// The value a field held before an applied update, as its result recorded it.
function recordedFrom(result: Record<string, unknown>, field: UpdateField) {
  const changes = result.changes
  if (!isRecord(changes)) {
    return undefined
  }
  const change = changes[field]
  if (!isRecord(change) || !('from' in change)) {
    return undefined
  }
  return change.from
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

// Each kind of change, by its `kind`, so a describer receives its own kind already narrowed.
type ChangeByKind = {
  [Change in ChangesetOperationChange as Change['kind']]: Change
}

type Describe<Kind extends ChangesetOperationKind> = (
  change: ChangeByKind[Kind],
  operation: ChangesetOperation,
  context: DescribeContext,
  t: ChangesetTranslate,
) => OperationDescription

// The lines of an edit: each field it sets, from what it held to what it will hold. Once
// applied, the value it replaced comes from the operation's result; before, from the item
// as it is now.
function describeUpdate(
  change: ChangeByKind['update-item'],
  operation: ChangesetOperation,
  context: DescribeContext,
  t: ChangesetTranslate,
) {
  const current = 'id' in change.item
    ? context.itemsById[change.item.id]
    : undefined
  const details: string[] = []
  for (const field of UPDATE_FIELDS) {
    if (!(field in change)) {
      continue
    }
    const label = t(updateFieldLabelKeys[field])
    const to = describeFieldValue(field, change[field], context, t)
    const applied = recordedFrom(operation.result, field)
    const from = applied === undefined
      ? current?.[field]
      : applied
    if (field === 'notes' || from === undefined) {
      details.push(t('describe.fieldTo', { field: label, to }))
      continue
    }
    details.push(t('describe.fieldFromTo', {
      field: label,
      from: describeFieldValue(field, from, context, t),
      to,
    }))
  }
  return details
}

// The names of `ids` in `names`, skipping any that are gone, joined for a sentence.
function joinNames(ids: string[], names: Record<string, string>) {
  return ids
    .map((id) => names[id])
    .filter(Boolean)
    .join(', ')
}

const describeByKind: { [Kind in ChangesetOperationKind]: Describe<Kind> } = {
  'create-item': (change, _operation, context, t) => {
    const details: string[] = []
    if (change.priority !== 'normal') {
      details.push(t('describe.fieldTo', {
        field: t(updateFieldLabelKeys.priority),
        to: describeFieldValue('priority', change.priority, context, t),
      }))
    }
    if (change.dueAt) {
      details.push(t('describe.fieldTo', {
        field: t(updateFieldLabelKeys.dueAt),
        to: context.formatInstant(change.dueAt),
      }))
    }
    const projects = joinNames(change.projectIds, context.projectNames)
    if (projects) {
      details.push(t('describe.inProjects', { names: projects }))
    }
    if (change.initiatives.length) {
      const names = change.initiatives
        .map((initiative) => describeTarget(initiative, 'initiative', context, t))
        .join(', ')
      details.push(t('describe.joinsInitiatives', { names }))
    }
    if (change.notes) {
      details.push(change.notes)
    }
    return { summary: t('describe.createItem', { title: change.title }), details }
  },
  'update-item': (change, operation, context, t) => ({
    summary: t('describe.updateItem', { item: describeTarget(change.item, 'item', context, t) }),
    details: describeUpdate(change, operation, context, t),
  }),
  'resolve-item': (change, _operation, context, t) => ({
    summary: t('describe.resolveItem', { item: describeTarget(change.item, 'item', context, t) }),
    details: [],
  }),
  'dismiss-item': (change, _operation, context, t) => ({
    summary: t('describe.dismissItem', { item: describeTarget(change.item, 'item', context, t) }),
    details: [],
  }),
  'comment': (change, _operation, context, t) => ({
    summary: t('describe.comment', { item: describeTarget(change.item, 'item', context, t) }),
    details: [ change.body ],
  }),
  'link': (change, _operation, context, t) => ({
    summary: t('describe.link', {
      item: describeTarget(change.item, 'item', context, t),
      provider: t(providerLabelKeys[change.target.provider], { ns: 'actionItems' }),
      reference: change.target.reference,
    }),
    details: [],
  }),
  'add-to-initiative': (change, _operation, context, t) => ({
    summary: t('describe.addToInitiative', {
      item: describeTarget(change.item, 'item', context, t),
      initiative: describeTarget(change.initiative, 'initiative', context, t),
    }),
    details: [],
  }),
  'remove-from-initiative': (change, _operation, context, t) => ({
    summary: t('describe.removeFromInitiative', {
      item: describeTarget(change.item, 'item', context, t),
      initiative: describeTarget(change.initiative, 'initiative', context, t),
    }),
    details: [],
  }),
  'create-initiative': (change, _operation, context, t) => {
    const details: string[] = []
    if (change.targetAt) {
      details.push(t('describe.fieldTo', {
        field: t('actionItems:history.fields.targetAt'),
        to: context.formatInstant(change.targetAt),
      }))
    }
    const projects = joinNames(change.projectIds, context.projectNames)
    if (projects) {
      details.push(t('describe.inProjects', { names: projects }))
    }
    if (change.description) {
      details.push(change.description)
    }
    return { summary: t('describe.createInitiative', { name: change.name }), details }
  },
}

// Calls the describer of the change's own kind. Taking the kind as its own argument lets
// TypeScript pair the describer with the change it narrows to.
function describeChange<Kind extends ChangesetOperationKind>(
  kind: Kind,
  change: ChangeByKind[Kind],
  operation: ChangesetOperation,
  context: DescribeContext,
  t: ChangesetTranslate,
) {
  const describe: Describe<Kind> | undefined = describeByKind[kind]
  if (!describe) {
    console.debug('describeOperation received a kind this build does not know', { operation })
    return { summary: kind, details: []}
  }
  return describe(change, operation, context, t)
}

// What an operation does, as a sentence and the lines worth showing under it. A kind this
// build does not know, from a newer API, is named as sent.
export function describeOperation(
  operation: ChangesetOperation,
  context: DescribeContext,
  t: ChangesetTranslate,
): OperationDescription {
  const change = operation.operation
  return describeChange(change.kind, change, operation, context, t)
}

// What an undo did not reverse, as sentences, from the operation's `undo`.
export function describeUndo(undo: OperationUndo, t: ChangesetTranslate) {
  const lines: string[] = []
  if (undo.refusal) {
    lines.push(t('undo.refusal', { reason: undo.refusal }))
  }
  if (undo.kept?.length) {
    const fields = undo.kept
      .map((field) => isUpdateField(field)
        ? t(updateFieldLabelKeys[field])
        : field)
      .join(', ')
    lines.push(t('undo.kept', { fields, count: undo.kept.length }))
  }
  if (undo.movedSince) {
    lines.push(t('undo.movedSince'))
  }
  if (undo.stillPosted) {
    lines.push(t('undo.stillPosted', {
      provider: t(providerLabelKeys[undo.stillPosted.provider], { ns: 'actionItems' }),
      key: undo.stillPosted.key,
    }))
  }
  // One line per provider, since an item can link issues in both.
  const closedKeysByProvider = new Map<LinkProvider, string[]>()
  for (const closed of undo.stillClosed ?? []) {
    const keys = closedKeysByProvider.get(closed.provider) ?? []
    keys.push(closed.key)
    closedKeysByProvider.set(closed.provider, keys)
  }
  for (const [ provider, keys ] of closedKeysByProvider) {
    lines.push(t('undo.stillClosed', {
      provider: t(providerLabelKeys[provider], { ns: 'actionItems' }),
      keys: keys.join(', '),
      count: keys.length,
    }))
  }
  return lines
}
