// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { GithubTokenKind } from '../../../api/routes/githubRoutes'

export const githubKindLabelKeys = {
  'classic': 'kinds.classic',
  'fine-grained': 'kinds.fine-grained',
} as const satisfies Record<GithubTokenKind, ParseKeys<'github'>>

// Each kind's words for the token field and for a token that is not written its way.
export const githubTokenLabelKeys = {
  'classic': { hint: 'form.tokenHint.classic', shapeError: 'form.errors.tokenShape.classic' },
  'fine-grained': {
    hint: 'form.tokenHint.fine-grained',
    shapeError: 'form.errors.tokenShape.fine-grained',
  },
} as const satisfies Record<GithubTokenKind, { hint: ParseKeys<'github'>, shapeError: ParseKeys<'github'> }>

// One setup step. The text is translated; URLs are not.
export type GithubSetupStep = {
  textKey: ParseKeys<'github'>
  link?: string
}

// Where each kind of token is created, in GitHub's own words.
export const githubSetupStepsByKind = {
  'classic': [
    { textKey: 'setup.classic.open', link: 'https://github.com/settings/tokens/new' },
    { textKey: 'setup.classic.name' },
    { textKey: 'setup.classic.scopes' },
    { textKey: 'setup.classic.generate' },
  ],
  'fine-grained': [
    {
      textKey: 'setup.fine-grained.open',
      link: 'https://github.com/settings/personal-access-tokens/new',
    },
    { textKey: 'setup.fine-grained.owner' },
    { textKey: 'setup.fine-grained.repositories' },
    { textKey: 'setup.fine-grained.permissions' },
    { textKey: 'setup.fine-grained.generate' },
  ],
} as const satisfies Record<GithubTokenKind, readonly GithubSetupStep[]>

// Whether a token is written the way GitHub writes this kind, mirroring `matches_kind` in
// api/src/routes/v1/github_credentials/mod.rs. Only the prefix and the alphabet are
// checked; the token itself is proven by asking GitHub.
export function matchesGithubTokenKind(kind: GithubTokenKind, token: string) {
  if (kind === 'classic') {
    // The 40 characters classic tokens had before GitHub gave them the ghp_ prefix.
    return /^ghp_[A-Za-z0-9]+$/.test(token) || /^[0-9a-fA-F]{40}$/.test(token)
  }
  return /^github_pat_\w+$/.test(token)
}
