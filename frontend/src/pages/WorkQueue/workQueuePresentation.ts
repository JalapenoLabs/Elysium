// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { IconType } from 'react-icons'
import type { WorkBatchPriority, WorkBatchState } from '../../api/routes/projectManagerRoutes'
import type {
  WorkBatchChanges,
  WorkBatchEventKind,
  WorkBatchHistoryEntry,
  WorkQueueEntry,
} from '../../api/routes/workQueueRoutes'

// User interface
import {
  LuArrowDownToLine,
  LuArrowUpToLine,
  LuCirclePause,
  LuCirclePlay,
  LuCircleX,
  LuGauge,
  LuZap,
} from 'react-icons/lu'

// Misc
import { WORK_BATCH_EVENT_KINDS } from '../../api/routes/workQueueRoutes'

// What people may do to a batch while it waits, and the words for a batch's history. Pure, and
// tested beside.

// Everything the queue offers to do to a batch.
export type WorkBatchAction =
  | 'runNext'
  | 'returnToPlace'
  | 'makeAsap'
  | 'makeNormal'
  | 'pause'
  | 'resume'
  | 'cancel'

// The change each action asks the API for; cancelling has a route of its own.
export const changesByAction = {
  runNext: { runNext: true },
  returnToPlace: { runNext: false },
  makeAsap: { priority: 'asap' },
  makeNormal: { priority: 'normal' },
  pause: { paused: true },
  resume: { paused: false },
} as const satisfies Record<Exclude<WorkBatchAction, 'cancel'>, WorkBatchChanges>

export const actionLabelKeys = {
  runNext: 'actions.runNext',
  returnToPlace: 'actions.returnToPlace',
  makeAsap: 'actions.makeAsap',
  makeNormal: 'actions.makeNormal',
  pause: 'actions.pause',
  resume: 'actions.resume',
  cancel: 'actions.cancel',
} as const satisfies Record<WorkBatchAction, ParseKeys<'workQueue'>>

export const actionIcons = {
  runNext: LuArrowUpToLine,
  returnToPlace: LuArrowDownToLine,
  makeAsap: LuZap,
  makeNormal: LuGauge,
  pause: LuCirclePause,
  resume: LuCirclePlay,
  cancel: LuCircleX,
} as const satisfies Record<WorkBatchAction, IconType>

export const actionDoneKeys = {
  runNext: 'actions.done.runNext',
  returnToPlace: 'actions.done.returnToPlace',
  makeAsap: 'actions.done.makeAsap',
  makeNormal: 'actions.done.makeNormal',
  pause: 'actions.done.pause',
  resume: 'actions.done.resume',
  cancel: 'actions.done.cancel',
} as const satisfies Record<WorkBatchAction, ParseKeys<'workQueue'>>

// What may be done to a batch in each state, before its priority and place narrow it. A batch a
// worker started on, or that ended, is no longer anyone's to change.
const actionsByState = {
  queued: [ 'runNext', 'makeAsap', 'pause', 'cancel' ],
  paused: [ 'resume', 'cancel' ],
  assigned: [ 'pause', 'cancel' ],
  running: [],
  succeeded: [],
  failed: [],
  cancelled: [],
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
  pinned: 'history.kinds.pinned',
  unpinned: 'history.kinds.unpinned',
  reprioritized: 'history.kinds.reprioritized',
  paused: 'history.kinds.paused',
  resumed: 'history.kinds.resumed',
  cancelled: 'history.kinds.cancelled',
  assigned: 'history.kinds.assigned',
  unassigned: 'history.kinds.unassigned',
  started: 'history.kinds.started',
  finished: 'history.kinds.finished',
} as const satisfies Record<WorkBatchEventKind, ParseKeys<'workQueue'>>

// One history entry as a line reads it: what happened, and what it carries beside it, such as
// the priority it moved from and to, or why it was cancelled. A kind this build does not know
// reads as its name, logged, rather than hiding the entry.
export function describeHistoryEntry(entry: WorkBatchHistoryEntry) {
  const kind = WORK_BATCH_EVENT_KINDS.find((known) => known === entry.kind)
  const labelKey = kind
    ? historyKindLabelKeys[kind]
    : null
  if (!labelKey) {
    console.debug('describeHistoryEntry met a history kind this build has no words for', {
      kind: entry.kind,
    })
  }

  const before = readPriority(entry.data.before)
  const after = readPriority(entry.data.after)
  const reason = typeof entry.data.reason === 'string'
    ? entry.data.reason
    : null
  const succeeded = typeof entry.data.succeeded === 'boolean'
    ? entry.data.succeeded
    : null
  return { labelKey, before, after, reason, succeeded } as const
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
