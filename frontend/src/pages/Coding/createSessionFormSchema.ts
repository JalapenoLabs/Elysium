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

// Elysium holds no SSH keys. The API clones a github.com SSH remote over HTTPS instead and
// refuses an SSH remote on any other host, since nothing could authenticate it. These are
// the prefixes the API recognizes as github.com (`Repository::from_url`).
const SSH_PREFIXES = [ 'ssh://', 'git@' ] as const
const GITHUB_SSH_PREFIXES = [ 'ssh://git@github.com/', 'git@github.com:' ] as const

// Built per render with `t` so validation messages are already translated.
export function createSessionFormSchema(t: TFunction<'coding'>) {
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
    repositoryUrl: z
      .string()
      .trim()
      .max(REPOSITORY_URL_MAX_CHARACTERS, { error: t('create.errors.repositoryUrlInvalid') })
      .refine(
        (value) => !value || REPOSITORY_URL_PATTERN.test(value),
        { error: t('create.errors.repositoryUrlInvalid') },
      )
      .refine(
        (value) => {
          const isSsh = SSH_PREFIXES.some((prefix) => value.startsWith(prefix))
          return !isSsh || GITHUB_SSH_PREFIXES.some((prefix) => value.startsWith(prefix))
        },
        { error: t('create.errors.repositoryUrlSshHost') },
      ),
    baseBranch: z
      .string()
      .trim()
      .max(BASE_BRANCH_MAX_CHARACTERS, { error: t('create.errors.baseBranchTooLong') }),
    prompt: z
      .string()
      .max(PROMPT_MAX_CHARACTERS),
    // `inherit` follows the project, `none` asks for no token, and anything else is a token id.
    githubChoice: z.string(),
  })
}

export type CreateSessionFormValues = z.infer<ReturnType<typeof createSessionFormSchema>>
