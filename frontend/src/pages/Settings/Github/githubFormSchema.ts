// Copyright © 2026 Jalapeno Labs

import type { TFunction } from 'i18next'
import type { GithubCredential } from '../../../api/routes/githubRoutes'

// Utility
import { z } from 'zod'

// Misc
import { GITHUB_TOKEN_KINDS } from '../../../api/routes/githubRoutes'
import { githubTokenLabelKeys, matchesGithubTokenKind } from './githubPresentation'

// Mirrors the API's limits so most mistakes are caught before a round trip.
const NAME_MAX_CHARACTERS = 120
const TOKEN_MAX_CHARACTERS = 255

// Built per render with `t` so validation messages are already translated. `stored` is the
// credential being edited, or null when adding one.
export function createGithubFormSchema(t: TFunction<'github'>, stored: GithubCredential | null) {
  return z.object({
    name: z
      .string()
      .trim()
      .min(1, { error: t('form.errors.nameRequired') })
      .max(NAME_MAX_CHARACTERS, { error: t('form.errors.nameTooLong') }),
    kind: z.enum(GITHUB_TOKEN_KINDS),
    token: z.string().trim(),
  }).superRefine((values, context) => {
    function fail(path: string, message: string) {
      context.addIssue({ code: 'custom', path: [ path ], message })
    }

    // A stored token is one kind of token, as the API enforces: changing the kind means
    // bringing the token that goes with it.
    if (!values.token) {
      if (!stored || stored.kind !== values.kind) {
        fail('token', t('form.errors.tokenRequired'))
      }
      return
    }

    if (values.token.length > TOKEN_MAX_CHARACTERS) {
      fail('token', t('form.errors.tokenTooLong'))
      return
    }
    if (!matchesGithubTokenKind(values.kind, values.token)) {
      fail('token', t(githubTokenLabelKeys[values.kind].shapeError))
    }
  })
}

export type GithubFormInput = z.input<ReturnType<typeof createGithubFormSchema>>
export type GithubFormValues = z.output<ReturnType<typeof createGithubFormSchema>>
