// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type {
  ProjectManagerStatus,
  ProjectManagerTurnStatus,
  ProjectManagerWake,
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
  queued: 'batches.states.queued',
  paused: 'batches.states.paused',
  assigned: 'batches.states.assigned',
  running: 'batches.states.running',
  succeeded: 'batches.states.succeeded',
  failed: 'batches.states.failed',
  cancelled: 'batches.states.cancelled',
} as const satisfies Record<WorkBatchState, ParseKeys<'projects'>>

export const batchStateChipColors = {
  queued: 'default',
  paused: 'warning',
  assigned: 'accent',
  running: 'accent',
  succeeded: 'success',
  failed: 'danger',
  cancelled: 'default',
} as const satisfies Record<WorkBatchState, ChipColor>

// What woke a turn, grouped as its row shows it: every message in full, oldest first, and how
// many records and finished batches came with them.
export function groupWakes(wakes: ProjectManagerWake[]) {
  const messages: ProjectManagerWake[] = []
  let records = 0
  let batches = 0
  for (const wake of wakes) {
    if (wake.kind === 'message') {
      messages.push(wake)
      continue
    }
    if (wake.kind === 'record') {
      records += 1
      continue
    }
    batches += 1
  }
  return { messages, records, batches } as const
}
