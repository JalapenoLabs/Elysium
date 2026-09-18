// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'

// Utility
import { z } from 'zod'

// Misc
import { findRepositoryConflict, MAX_SESSION_REPOSITORIES } from './sessionRepositories'

// Mirrors the API's limits so most mistakes are caught before a round trip.
export const SESSION_TITLE_MAX_CHARACTERS = 200
const REPOSITORY_URL_MAX_CHARACTERS = 2048
const BASE_BRANCH_MAX_CHARACTERS = 255
const PROMPT_MAX_CHARACTERS = 100_000

// The API clones into a directory named after the URL's last segment, and accepts
// only the git URL forms a satellite can fetch.
const REPOSITORY_URL_PATTERN = /^(https?:\/\/|ssh:\/\/|git@)\S+\/[\w.-]+?(\.git)?\/?$/

// Elysium holds no SSH keys. The API clones a github.com SSH remote over HTTPS instead and
// refuses an SSH remote on any other host, since nothing could authenticate it. These are
// the prefixes the API recognizes as github.com (`Repository::from_url`).
const SSH_PREFIXES = [ 'ssh://', 'git@' ] as const
const GITHUB_SSH_PREFIXES = [ 'ssh://git@github.com/', 'git@github.com:' ] as const

// One repository URL, as the API's `validate_repository_url` accepts it. Shared by the
// Add by URL field, which checks a URL before it becomes a row.
export function createRepositoryUrlSchema(t: TFunction<'coding'>) {
  return z
    .string()
    .trim()
    .max(REPOSITORY_URL_MAX_CHARACTERS, { error: t('create.errors.repositoryUrlInvalid') })
    .regex(REPOSITORY_URL_PATTERN, { error: t('create.errors.repositoryUrlInvalid') })
    .refine(
      (value) => {
        const isSsh = SSH_PREFIXES.some((prefix) => value.startsWith(prefix))
        return !isSsh || GITHUB_SSH_PREFIXES.some((prefix) => value.startsWith(prefix))
      },
      { error: t('create.errors.repositoryUrlSshHost') },
    )
}

type SessionFormOptions = {
  // A session started from an action item needs a prompt, as the API requires.
  isPromptRequired: boolean
}

// Built per render with `t` so validation messages are already translated.
export function createSessionFormSchema(t: TFunction<'coding'>, options: SessionFormOptions) {
  const prompt = z
    .string()
    .max(PROMPT_MAX_CHARACTERS)

  return z.object({
    projectId: z
      .string()
      .min(1, { error: t('create.errors.projectRequired') }),
    satelliteId: z
      .string()
      .min(1, { error: t('create.errors.satelliteRequired') }),
    // Optional here: a blank title falls back to the prompt's first line on submit.
    title: z
      .string()
      .trim()
      .max(SESSION_TITLE_MAX_CHARACTERS, { error: t('create.errors.titleTooLong') }),
    repositories: z
      .array(z.object({
        url: createRepositoryUrlSchema(t),
        baseBranch: z
          .string()
          .trim()
          .max(BASE_BRANCH_MAX_CHARACTERS, { error: t('create.errors.baseBranchTooLong') }),
        // owner/name when picked from the token's list; null when added by URL.
        fullName: z.string().nullable(),
      }))
      .max(MAX_SESSION_REPOSITORIES, {
        error: t('create.repositories.errors.tooMany', { max: MAX_SESSION_REPOSITORIES }),
      })
      .superRefine((repositories, context) => {
        const conflict = findRepositoryConflict(repositories.map((repository) => repository.url))
        if (!conflict) {
          return
        }
        context.addIssue({
          code: 'custom',
          message: conflict.kind === 'duplicate'
            ? t('create.repositories.errors.duplicate', conflict)
            : t('create.repositories.errors.directory', conflict),
        })
      }),
    prompt: options.isPromptRequired
      ? prompt.refine((value) => value.trim().length > 0, { error: t('create.errors.promptRequired') })
      : prompt,
    // `inherit` follows the project, `none` asks for no token, and anything else is a token id.
    githubChoice: z.string(),
  })
}

export type CreateSessionFormValues = z.infer<ReturnType<typeof createSessionFormSchema>>
export type SessionRepositoryValue = CreateSessionFormValues['repositories'][number]
