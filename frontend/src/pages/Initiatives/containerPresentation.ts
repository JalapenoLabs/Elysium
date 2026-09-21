// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { LinkProvider } from '../../api/routes/actionItemRoutes'
import type { GithubRepository } from '../../api/routes/githubRoutes'
import type { ContainerKind, ContainerTarget } from '../../api/routes/initiativeRoutes'

// Misc
import { fromCredentialKey } from '../ActionItems/linkPresentation'

// Names for an initiative's containers and what a container picker holds. Pure, so the
// initiative page and its picker share one spelling of each.

export const containerKindLabelKeys = {
  epic: 'containers.kinds.epic',
  filter: 'containers.kinds.filter',
  milestone: 'containers.kinds.milestone',
  label: 'containers.kinds.label',
} as const satisfies Record<ContainerKind, ParseKeys<'initiatives'>>

// The containers each provider has, the first being where a picker starts.
export const containerKindsByProvider = {
  jira: [ 'epic', 'filter' ],
  github: [ 'milestone', 'label' ],
} as const satisfies Record<LinkProvider, readonly ContainerKind[]>

export type ContainerTargetDraft = {
  // A key from `toCredentialKey`, or empty until one is chosen.
  credentialKey: string
  kind: ContainerKind
  // GitHub's milestones and labels belong to a repository; Jira's containers do not.
  repository: GithubRepository | null
  // An epic's key, a filter's id, owner/name#3, or owner/name:label; null until picked.
  reference: string | null
}

export const EMPTY_CONTAINER_TARGET_DRAFT: ContainerTargetDraft = {
  credentialKey: '',
  kind: 'epic',
  repository: null,
  reference: null,
}

// The container a draft names, or null while it names none, or names a kind its provider
// does not have.
export function toContainerTarget(draft: ContainerTargetDraft): ContainerTarget | null {
  if (!draft.reference) {
    return null
  }
  const credential = fromCredentialKey(draft.credentialKey)
  if (!credential) {
    return null
  }
  const kinds: readonly ContainerKind[] = containerKindsByProvider[credential.provider]
  if (!kinds.includes(draft.kind)) {
    console.debug('A container draft names a kind its provider does not have', { draft })
    return null
  }
  return {
    provider: credential.provider,
    credentialId: credential.credentialId,
    kind: draft.kind,
    reference: draft.reference,
  }
}
