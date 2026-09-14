// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Mirrors the API's limits so most mistakes are caught before a round trip.
export const SESSION_TITLE_MAX_CHARACTERS = 200
const REPOSITORY_URL_MAX_CHARACTERS = 2048
const BASE_BRANCH_MAX_CHARACTERS = 255
const PROMPT_MAX_CHARACTERS = 100_000

// The API clones into a directory named after the URL's last segment, and accepts
// only the git URL forms a satellite can fetch.
const REPOSITORY_URL_PATTERN = /^(https?:\/\/|ssh:\/\/|git@)\S+\/[\w.-]+?(\.git)?\/?$/

// Built per render with `t` so validation messages are already translated.
export function createSessionFormSchema(t: TFunction<'coding'>) {
  return z.object({
    satelliteId: z
      .string()
      .min(1, { error: t('create.errors.satelliteRequired') }),
    // Optional here: a blank title falls back to the prompt's first line on submit.
    title: z
      .string()
      .trim()
      .max(SESSION_TITLE_MAX_CHARACTERS, { error: t('create.errors.titleTooLong') }),
    repositoryUrl: z
      .string()
      .trim()
      .max(REPOSITORY_URL_MAX_CHARACTERS, { error: t('create.errors.repositoryUrlInvalid') })
      .refine(
        (value) => !value || REPOSITORY_URL_PATTERN.test(value),
        { error: t('create.errors.repositoryUrlInvalid') },
      ),
    baseBranch: z
      .string()
      .trim()
      .max(BASE_BRANCH_MAX_CHARACTERS, { error: t('create.errors.baseBranchTooLong') }),
    prompt: z
      .string()
      .max(PROMPT_MAX_CHARACTERS),
  })
}

export type CreateSessionFormValues = z.infer<ReturnType<typeof createSessionFormSchema>>
