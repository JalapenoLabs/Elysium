// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { WorkBatch, WorkBatchPriority } from '../../api/routes/projectManagerRoutes'
import type { WorkBatchEdits } from '../../api/routes/workQueueRoutes'

// Utility
import { z } from 'zod'

// Misc
import { BASE_BRANCH_MAX_CHARACTERS, createRepositoryUrlSchema } from '../Coding/createSessionFormSchema'
import { findRepositoryConflict, MAX_SESSION_REPOSITORIES } from '../Coding/sessionRepositories'

// A proposed batch as its edit page holds it, and the edits approving it sends. Mirrors the rules
// the API holds every batch to (`check_edits` in api/src/project_managers/batches.rs), so most
// mistakes show before a round trip. Pure, and tested beside.

// Mirrors `TITLE_MAX_CHARACTERS` and `TEXT_MAX_CHARACTERS` in the same file.
export const BATCH_TITLE_MAX_CHARACTERS = 200
export const BATCH_GOAL_MAX_CHARACTERS = 20_000

// How far ahead a batch may be scheduled, mirroring `SCHEDULE_HORIZON` in
// api/src/models/work_batch.rs: further than a year is a plan, not a batch.
const SCHEDULE_HORIZON_MS = 365 * 24 * 60 * 60 * 1000

// Built per render with `t` so messages are already translated, and with `now`, since a scheduled
// time must still be ahead of it.
export function createWorkBatchEditSchema(
  t: TFunction<'workQueue'>,
  codingT: TFunction<'coding'>,
  now: number,
) {
  return z.object({
    title: z
      .string()
      .trim()
      .min(1, { error: t('edit.errors.titleRequired') })
      .max(BATCH_TITLE_MAX_CHARACTERS, { error: t('edit.errors.titleTooLong', { max: BATCH_TITLE_MAX_CHARACTERS }) }),
    goal: z
      .string()
      .trim()
      .min(1, { error: t('edit.errors.goalRequired') })
      .max(BATCH_GOAL_MAX_CHARACTERS, { error: t('edit.errors.goalTooLong', { max: BATCH_GOAL_MAX_CHARACTERS }) }),
    repositories: z
      .array(z.object({
        url: createRepositoryUrlSchema(codingT),
        baseBranch: z
          .string()
          .trim()
          .max(BASE_BRANCH_MAX_CHARACTERS, { error: codingT('create.errors.baseBranchTooLong') }),
      }))
      .max(MAX_SESSION_REPOSITORIES, {
        error: codingT('create.repositories.errors.tooMany', { max: MAX_SESSION_REPOSITORIES }),
      })
      .superRefine((repositories, context) => {
        // The API refuses the same repository twice and two that would clone into one directory.
        const conflict = findRepositoryConflict(repositories.map((repository) => repository.url.trim()))
        if (!conflict) {
          return
        }
        context.addIssue({
          code: 'custom',
          message: conflict.kind === 'duplicate'
            ? codingT('create.repositories.errors.duplicate', conflict)
            : codingT('create.repositories.errors.directory', conflict),
        })
      }),
    priority: z.enum([ 'asap', 'normal', 'scheduled' ]),
    // An instant with its offset, set exactly while the priority is scheduled.
    scheduledFor: z.string().nullable(),
  })
    .superRefine((values, context) => {
      if (values.priority !== 'scheduled') {
        return
      }
      const at = values.scheduledFor
        ? Date.parse(values.scheduledFor)
        : Number.NaN
      if (Number.isNaN(at)) {
        context.addIssue({ code: 'custom', path: [ 'scheduledFor' ], message: t('edit.errors.scheduleRequired') })
        return
      }
      if (at <= now) {
        context.addIssue({ code: 'custom', path: [ 'scheduledFor' ], message: t('edit.errors.schedulePast') })
        return
      }
      if (at > now + SCHEDULE_HORIZON_MS) {
        context.addIssue({ code: 'custom', path: [ 'scheduledFor' ], message: t('edit.errors.scheduleTooFar') })
      }
    })
}

type WorkBatchEditSchema = ReturnType<typeof createWorkBatchEditSchema>
export type WorkBatchEditValues = z.input<WorkBatchEditSchema>
export type WorkBatchEditOutput = z.output<WorkBatchEditSchema>

// The batch as the form holds it: every field text, a missing base branch blank.
export function toWorkBatchEditValues(batch: WorkBatch): WorkBatchEditValues {
  return {
    title: batch.title,
    goal: batch.goal,
    repositories: batch.repositories.map((repository) => ({
      url: repository.url,
      baseBranch: repository.baseBranch ?? '',
    })),
    priority: batch.priority,
    scheduledFor: batch.scheduledFor,
  }
}

// What approving sends: only the fields the person changed, so the rest stays as its Project
// Manager proposed it, and an untouched batch sends nothing to change. A blank base branch is
// none, and a priority and its time travel together, as the API takes them.
export function toWorkBatchEdits(batch: WorkBatch, output: WorkBatchEditOutput): WorkBatchEdits {
  const edits: WorkBatchEdits = {}
  if (output.title !== batch.title) {
    edits.title = output.title
  }
  if (output.goal !== batch.goal) {
    edits.goal = output.goal
  }

  const repositories = output.repositories.map((repository) => repository.baseBranch
    ? { url: repository.url, baseBranch: repository.baseBranch }
    : { url: repository.url })
  const proposed = batch.repositories.map((repository) => repository.baseBranch
    ? { url: repository.url, baseBranch: repository.baseBranch }
    : { url: repository.url })
  if (JSON.stringify(repositories) !== JSON.stringify(proposed)) {
    edits.repositories = repositories
  }

  const scheduledFor = output.priority === 'scheduled'
    ? output.scheduledFor
    : null
  const isSameTime = scheduledFor === batch.scheduledFor
    || (scheduledFor !== null && batch.scheduledFor !== null
      && Date.parse(scheduledFor) === Date.parse(batch.scheduledFor))
  if (output.priority !== batch.priority || !isSameTime) {
    edits.priority = output.priority satisfies WorkBatchPriority
    if (scheduledFor) {
      edits.scheduledFor = scheduledFor
    }
  }
  return edits
}
