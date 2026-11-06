// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'
import type {
  WorkBatch,
  WorkBatchPriority,
  WorkBatchState,
  WorkBatchWait,
} from '../../api/routes/projectManagerRoutes'
import type {
  BatchPullRequest,
  WorkBatchChanges,
  WorkBatchEventKind,
  WorkBatchHistoryEntry,
  WorkQueueEntry,
} from '../../api/routes/workQueueRoutes'

// User interface
import {
  LuArrowDownToLine,
  LuArrowUpToLine,
  LuBan,
  LuCircleCheck,
  LuCirclePause,
  LuCirclePlay,
  LuCircleX,
  LuGauge,
  LuZap,
} from 'react-icons/lu'

// Misc
import { WORK_BATCH_WAITS } from '../../api/routes/projectManagerRoutes'
import { WORK_BATCH_EVENT_KINDS } from '../../api/routes/workQueueRoutes'

// What people may do to a batch while it waits, and the words for a batch's history. Pure, and
// tested beside.

// Everything the queue offers to do to a batch. Approving and rejecting are for a proposed batch
// alone, and only a person does either.
export type WorkBatchAction =
  | 'approve'
  | 'reject'
  | 'runNext'
  | 'returnToPlace'
  | 'makeAsap'
  | 'makeNormal'
  | 'pause'
  | 'resume'
  | 'cancel'

// The actions that each have a route of their own rather than being a change to the batch.
export type WorkBatchDecision = 'approve' | 'reject' | 'cancel'

// The change each action asks the API for.
export const changesByAction = {
  runNext: { runNext: true },
  returnToPlace: { runNext: false },
  makeAsap: { priority: 'asap' },
  makeNormal: { priority: 'normal' },
  pause: { paused: true },
  resume: { paused: false },
} as const satisfies Record<Exclude<WorkBatchAction, WorkBatchDecision>, WorkBatchChanges>

export const actionLabelKeys = {
  approve: 'actions.approve',
  reject: 'actions.reject',
  runNext: 'actions.runNext',
  returnToPlace: 'actions.returnToPlace',
  makeAsap: 'actions.makeAsap',
  makeNormal: 'actions.makeNormal',
  pause: 'actions.pause',
  resume: 'actions.resume',
  cancel: 'actions.cancel',
} as const satisfies Record<WorkBatchAction, ParseKeys<'workQueue'>>

export const actionIcons = {
  approve: LuCircleCheck,
  reject: LuBan,
  runNext: LuArrowUpToLine,
  returnToPlace: LuArrowDownToLine,
  makeAsap: LuZap,
  makeNormal: LuGauge,
  pause: LuCirclePause,
  resume: LuCirclePlay,
  cancel: LuCircleX,
} as const satisfies Record<WorkBatchAction, IconType>

export const actionDoneKeys = {
  approve: 'actions.done.approve',
  reject: 'actions.done.reject',
  runNext: 'actions.done.runNext',
  returnToPlace: 'actions.done.returnToPlace',
  makeAsap: 'actions.done.makeAsap',
  makeNormal: 'actions.done.makeNormal',
  pause: 'actions.done.pause',
  resume: 'actions.done.resume',
  cancel: 'actions.done.cancel',
} as const satisfies Record<WorkBatchAction, ParseKeys<'workQueue'>>

// The actions that end what they act on for good, which show in red, last, below the rest.
export const DANGEROUS_ACTIONS: readonly WorkBatchAction[] = [ 'reject', 'cancel' ]

// What may be done to a batch in each state, before its priority and place narrow it. A proposal
// is only approved or rejected; a batch a worker started on, or that ended, is no longer anyone's
// to change.
const actionsByState = {
  proposed: [ 'approve', 'reject' ],
  queued: [ 'runNext', 'makeAsap', 'pause', 'cancel' ],
  paused: [ 'resume', 'cancel' ],
  assigned: [ 'pause', 'cancel' ],
  running: [],
  succeeded: [],
  failed: [],
  cancelled: [],
  rejected: [],
} as const satisfies Record<WorkBatchState, readonly WorkBatchAction[]>

// The actions `entry` offers at `now`, in the order its row and page show them. A batch asked to
// run next offers its place back instead; an ASAP batch offers normal instead of ASAP; and one
// scheduled for later cannot run next, but can run as soon as possible.
export function availableActions(
  entry: Pick<WorkQueueEntry, 'state' | 'priority' | 'scheduledFor' | 'pinnedAt'>,
  now: number,
): WorkBatchAction[] {
  const isLater = entry.priority === 'scheduled'
    && entry.scheduledFor !== null
    && Date.parse(entry.scheduledFor) > now
  const available: WorkBatchAction[] = []
  for (const action of actionsByState[entry.state]) {
    if (action === 'runNext') {
      if (entry.pinnedAt) {
        available.push('returnToPlace')
        continue
      }
      if (isLater) {
        continue
      }
      available.push(action)
      continue
    }
    if (action === 'makeAsap' && entry.priority === 'asap') {
      available.push('makeNormal')
      continue
    }
    available.push(action)
  }
  return available
}

export const historyKindLabelKeys = {
  submitted: 'history.kinds.submitted',
  held_for_approval: 'history.kinds.held_for_approval',
  edited: 'history.kinds.edited',
  approved: 'history.kinds.approved',
  rejected: 'history.kinds.rejected',
  pinned: 'history.kinds.pinned',
  unpinned: 'history.kinds.unpinned',
  reprioritized: 'history.kinds.reprioritized',
  paused: 'history.kinds.paused',
  resumed: 'history.kinds.resumed',
  cancelled: 'history.kinds.cancelled',
  assigned: 'history.kinds.assigned',
  unassigned: 'history.kinds.unassigned',
  started: 'history.kinds.started',
  waiting: 'history.kinds.waiting',
  answered: 'history.kinds.answered',
  finished: 'history.kinds.finished',
} as const satisfies Record<WorkBatchEventKind, ParseKeys<'workQueue'>>

// A `waiting` entry's line for what it waits on, with when its worker looks again.
const waitingHistoryKeys = {
  review: 'history.waiting.review',
  ci: 'history.waiting.ci',
  question: 'history.waiting.question',
} as const satisfies Record<WorkBatchWait, ParseKeys<'workQueue'>>

// The batch page's heading for what its worker waits on.
export const waitHeadingKeys = {
  review: 'batch.waiting.review',
  ci: 'batch.waiting.ci',
  question: 'batch.waiting.question',
} as const satisfies Record<WorkBatchWait, ParseKeys<'workQueue'>>

// When its worker looks again: a question answered first wakes it sooner.
export const waitUntilKeys = {
  review: 'batch.waiting.until',
  ci: 'batch.waiting.until',
  question: 'batch.waiting.untilAnswered',
} as const satisfies Record<WorkBatchWait, ParseKeys<'workQueue'>>

// What a batch's worker waits on now, or null when it waits on nothing. The API sets the wait
// only while a running batch's worker keeps its container, so any other state holds none.
export function currentWait(batch: Pick<WorkBatch, 'state' | 'waitingOn' | 'waitingDetail' | 'waitingUntil'>) {
  if (batch.state !== 'running' || !batch.waitingOn) {
    return null
  }
  return {
    on: batch.waitingOn,
    detail: batch.waitingDetail,
    until: batch.waitingUntil,
  } as const
}

// The fields a person may change as they approve a proposal, as an edited entry names them; a
// priority and its scheduled time change together, so they read as one.
export const EDITED_FIELDS = [ 'title', 'goal', 'repositories', 'priority' ] as const
export type EditedField = typeof EDITED_FIELDS[number]

export const editedFieldLabelKeys = {
  title: 'history.edited.title',
  goal: 'history.edited.goal',
  repositories: 'history.edited.repositories',
  priority: 'history.edited.priority',
} as const satisfies Record<EditedField, ParseKeys<'workQueue'>>

// One history entry as a line reads it: what happened, and what it carries beside it, such as
// the priority it moved from and to, the fields a person changed, why it was cancelled or
// rejected, what its worker waits for, or the answer it was given. A kind this build does not
// know reads as its name, logged, rather than hiding the entry.
export function describeHistoryEntry(entry: WorkBatchHistoryEntry) {
  const kind = WORK_BATCH_EVENT_KINDS.find((known) => known === entry.kind)
  if (!kind) {
    console.debug('describeHistoryEntry met a history kind this build has no words for', {
      kind: entry.kind,
    })
  }

  // A wait whose kind or time this build cannot read still says its worker waits.
  const wait = kind === 'waiting'
    ? readWait(entry.data)
    : null
  let labelKey: ParseKeys<'workQueue'> | null = null
  if (wait) {
    labelKey = waitingHistoryKeys[wait.on]
  }
  else if (kind) {
    labelKey = historyKindLabelKeys[kind]
  }

  const before = readPriority(entry.data.before)
  const after = readPriority(entry.data.after)
  const succeeded = typeof entry.data.succeeded === 'boolean'
    ? entry.data.succeeded
    : null
  const edited = kind === 'edited'
    ? readEditedFields(entry.data.after)
    : []
  // The words beneath the line: why it was cancelled or rejected, what its worker waits for, or
  // the answer.
  let note: string | null = null
  for (const field of [ 'reason', 'detail', 'answer' ] as const) {
    const value = entry.data[field]
    if (typeof value === 'string' && value.trim()) {
      note = value
      break
    }
  }
  return {
    labelKey,
    before,
    after,
    note,
    succeeded,
    edited,
    until: wait?.until ?? null,
  } as const
}

// The fields an edited entry's `after` holds, in the order a batch lists them. A field this build
// does not know is left out, logged, rather than named wrong.
function readEditedFields(after: unknown) {
  if (!after || typeof after !== 'object') {
    console.debug('describeHistoryEntry met an edited entry without its fields', { after })
    return []
  }
  const named = Object.keys(after)
  const fields: EditedField[] = []
  for (const field of EDITED_FIELDS) {
    if (named.includes(field)) {
      fields.push(field)
    }
  }
  return fields
}

// Where a pull request a batch's worker recorded stands, in words and a chip's color: who merged
// it, that it closed unmerged, or that it is open, with why the last look left it so.
export function pullRequestStanding(pullRequest: BatchPullRequest) {
  if (pullRequest.state === 'merged' && pullRequest.mergedByElysium) {
    return {
      labelKey: 'batch.pullRequests.mergedByElysium',
      color: 'success',
      login: null,
      note: null,
    } as const
  }
  if (pullRequest.state === 'merged') {
    return {
      labelKey: 'batch.pullRequests.mergedBy',
      color: 'success',
      login: pullRequest.mergedBy ?? '',
      note: null,
    } as const
  }
  if (pullRequest.state === 'closed') {
    return {
      labelKey: 'batch.pullRequests.closed',
      color: 'default',
      login: null,
      note: null,
    } as const
  }
  return {
    labelKey: 'batch.pullRequests.open',
    color: 'accent',
    login: null,
    note: pullRequest.heldBecause,
  } as const
}

// The `{ on, until }` a waiting entry carries, or null when either is not one this build reads.
function readWait(data: Record<string, unknown>) {
  const on = WORK_BATCH_WAITS.find((known) => known === data.on)
  const until = data.until
  if (!on || typeof until !== 'string' || Number.isNaN(Date.parse(until))) {
    console.debug('describeHistoryEntry met a wait it cannot read', { on: data.on, until })
    return null
  }
  return { on, until } as const
}

// A `{ priority, scheduledFor }` a reprioritized entry carries, or null when it carries none.
function readPriority(value: unknown) {
  if (!value || typeof value !== 'object' || !('priority' in value)) {
    return null
  }
  const priority = value.priority
  if (priority !== 'asap' && priority !== 'normal' && priority !== 'scheduled') {
    return null
  }
  const scheduledFor = 'scheduledFor' in value && typeof value.scheduledFor === 'string'
    ? value.scheduledFor
    : null
  return { priority: priority satisfies WorkBatchPriority, scheduledFor } as const
}
