// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { WorkerSetup, WorkerSetupRequest } from '../../api/routes/workerSetupRoutes'

// Utility
import { z } from 'zod'

// Misc
import { numberFormat } from '../../format/intl'
import { BASE_BRANCH_MAX_CHARACTERS, createRepositoryUrlSchema } from '../Coding/createSessionFormSchema'
import { findRepositoryConflict } from '../Coding/sessionRepositories'

// The most repositories a project declares, mirroring `REPOSITORIES_MAX` in
// api/src/models/worker_setup.rs: a batch clones at most this many.
export const MAX_WORKER_REPOSITORIES = 16

// The longest script, mirroring `SCRIPT_MAX_CHARACTERS` in the same file.
export const WORKER_SCRIPT_MAX_CHARACTERS = 20_000

// A project's worker setup as its Workers page edits it, mirroring the API's rules
// (`check` in api/src/routes/v1/projects/worker_setup.rs) so most mistakes show before a round
// trip. Repository URLs follow a coding session's rules, so their words are the Coding page's.
export function createWorkerSetupSchema(t: TFunction<'projects'>, codingT: TFunction<'coding'>) {
  // A script is kept exactly as written, whitespace included, so it is never trimmed. One that
  // is only whitespace is no script, so the limit applies to the rest. Counted by code point,
  // as the API counts characters.
  const script = z
    .string()
    .refine(
      (value) => !value.trim() || Array.from(value).length <= WORKER_SCRIPT_MAX_CHARACTERS,
      { error: t('workers.errors.scriptTooLong', { max: numberFormat({}).format(WORKER_SCRIPT_MAX_CHARACTERS) }) },
    )

  return z.object({
    toolsSetupScript: script,
    repositories: z
      .array(z.object({
        url: createRepositoryUrlSchema(codingT),
        baseBranch: z
          .string()
          .trim()
          .max(BASE_BRANCH_MAX_CHARACTERS, { error: codingT('create.errors.baseBranchTooLong') }),
        setupScript: script,
      }))
      .max(MAX_WORKER_REPOSITORIES, { error: t('workers.errors.tooMany', { max: MAX_WORKER_REPOSITORIES }) })
      .superRefine((repositories, context) => {
        // The API refuses the same repository twice and two that would clone into one directory.
        const urls = repositories.map((repository) => repository.url.trim())
        const conflict = findRepositoryConflict(urls)
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
  })
}

type WorkerSetupSchema = ReturnType<typeof createWorkerSetupSchema>
export type WorkerSetupFormValues = z.input<WorkerSetupSchema>
export type WorkerSetupFormOutput = z.output<WorkerSetupSchema>
export type WorkerRepositoryValue = WorkerSetupFormValues['repositories'][number]

// The setup as the form holds it: every field text, a missing one blank.
export function toWorkerSetupFormValues(setup: WorkerSetup): WorkerSetupFormValues {
  return {
    toolsSetupScript: setup.toolsSetupScript ?? '',
    repositories: setup.repositories.map((repository) => ({
      url: repository.url,
      baseBranch: repository.baseBranch ?? '',
      setupScript: repository.setupScript ?? '',
    })),
  }
}

// The form's checked values as the API replaces the setup with them. The schema already
// trimmed each URL and base branch; a blank one is none, as is a script of only whitespace.
export function toWorkerSetupRequest(values: WorkerSetupFormOutput): WorkerSetupRequest {
  return {
    toolsSetupScript: values.toolsSetupScript.trim()
      ? values.toolsSetupScript
      : null,
    repositories: values.repositories.map((repository) => ({
      url: repository.url,
      baseBranch: repository.baseBranch || null,
      setupScript: repository.setupScript.trim()
        ? repository.setupScript
        : null,
    })),
  }
}
