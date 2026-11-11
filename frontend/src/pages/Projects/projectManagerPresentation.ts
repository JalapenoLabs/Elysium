// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { PersonPromise, PromiseState } from '../../api/routes/promiseRoutes'
import type {
  ProjectManagerStatus,
  ProjectManagerTurnStatus,
  ProjectManagerWake,
  WorkBatch,
  WorkBatchOrigin,
  WorkBatchPriority,
  WorkBatchState,
} from '../../api/routes/projectManagerRoutes'

// Names, colors, and groupings for a project's Project Manager and its batches. Pure, and
// tested beside.

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const managerStatusLabelKeys = {
  running: 'manager.statuses.running',
  held: 'manager.statuses.held',
  errored: 'manager.statuses.errored',
  waiting: 'manager.statuses.waiting',
  idle: 'manager.statuses.idle',
} as const satisfies Record<ProjectManagerStatus, ParseKeys<'projects'>>

export const managerStatusChipColors = {
  running: 'accent',
  held: 'warning',
  errored: 'danger',
  waiting: 'default',
  idle: 'success',
} as const satisfies Record<ProjectManagerStatus, ChipColor>

export const turnStatusLabelKeys = {
  running: 'manager.turn.statuses.running',
  succeeded: 'manager.turn.statuses.succeeded',
  failed: 'manager.turn.statuses.failed',
} as const satisfies Record<ProjectManagerTurnStatus, ParseKeys<'projects'>>

export const batchPriorityLabelKeys = {
  asap: 'batches.priorities.asap',
  normal: 'batches.priorities.normal',
  scheduled: 'batches.priorities.scheduled',
} as const satisfies Record<WorkBatchPriority, ParseKeys<'projects'>>

export const batchPriorityChipColors = {
  asap: 'warning',
  normal: 'default',
  scheduled: 'accent',
} as const satisfies Record<WorkBatchPriority, ChipColor>

export const batchStateLabelKeys = {
  proposed: 'batches.states.proposed',
  queued: 'batches.states.queued',
  paused: 'batches.states.paused',
  assigned: 'batches.states.assigned',
  running: 'batches.states.running',
  succeeded: 'batches.states.succeeded',
  failed: 'batches.states.failed',
  cancelled: 'batches.states.cancelled',
  rejected: 'batches.states.rejected',
} as const satisfies Record<WorkBatchState, ParseKeys<'projects'>>

// A proposal waits on the person, so it stands out as a paused batch does; a rejection was their
// decision, not a failure.
export const batchStateChipColors = {
  proposed: 'warning',
  queued: 'default',
  paused: 'warning',
  assigned: 'accent',
  running: 'accent',
  succeeded: 'success',
  failed: 'danger',
  cancelled: 'default',
  rejected: 'default',
} as const satisfies Record<WorkBatchState, ChipColor>

export const batchOriginLabelKeys = {
  human_rooted: 'batches.origins.human_rooted',
  found: 'batches.origins.found',
} as const satisfies Record<WorkBatchOrigin, ParseKeys<'projects'>>

// Found work stands out: nobody asked for it, so a person weighs whether it is wanted.
export const batchOriginChipColors = {
  human_rooted: 'default',
  found: 'warning',
} as const satisfies Record<WorkBatchOrigin, ChipColor>

export const promiseStateLabelKeys = {
  waiting: 'promises.states.waiting',
  due: 'promises.states.due',
  drafted: 'promises.states.drafted',
  kept: 'promises.states.kept',
  dismissed: 'promises.states.dismissed',
} as const satisfies Record<PromiseState, ParseKeys<'projects'>>

export const promiseStateChipColors = {
  waiting: 'default',
  due: 'warning',
  drafted: 'accent',
  kept: 'success',
  dismissed: 'default',
} as const satisfies Record<PromiseState, ChipColor>

// The work a promise waits on, as its row names it: a batch of the project's by its title, an
// action item by its title with a link to it, or none yet. A batch or item this page does not
// hold is named by kind alone.
export function promiseWork(
  promise: PersonPromise,
  batches: WorkBatch[],
  itemTitles: Record<string, string>,
) {
  if (promise.workBatchId) {
    const batch = batches.find((candidate) => candidate.id === promise.workBatchId)
    return { kind: 'batch', title: batch?.title ?? null, itemId: null } as const
  }
  if (promise.workItemId) {
    return { kind: 'item', title: itemTitles[promise.workItemId] ?? null, itemId: promise.workItemId } as const
  }
  return { kind: 'none', title: null, itemId: null } as const
}

// What woke a turn, grouped as its row shows it: every message in full, oldest first, and how
// many records, finished batches, promises, edits to the brief, and assigned items came with them.
export function groupWakes(wakes: ProjectManagerWake[]) {
  const messages: ProjectManagerWake[] = []
  const counts = { record: 0, batch: 0, promise: 0, brief: 0, assignment: 0 }
  for (const wake of wakes) {
    if (wake.kind === 'message') {
      messages.push(wake)
      continue
    }
    counts[wake.kind] += 1
  }
  return {
    messages,
    records: counts.record,
    batches: counts.batch,
    promises: counts.promise,
    briefEdits: counts.brief,
    assignments: counts.assignment,
  } as const
}
