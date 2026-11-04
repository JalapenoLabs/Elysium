// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { LinkProvider } from '../../api/routes/actionItemRoutes'
import type { Connector } from '../../api/routes/connectorRoutes'
import type { ProjectTracker, TrackerSource } from '../../api/routes/projectManagerRoutes'
import type { PickerOption } from '../../components/MultiPicker'

// Where a project's Project Manager files issues, in words, and the tracker picker's rules.
// Pure, and tested beside. See docs/engine.md, Project Managers.

// A tracker on GitHub names no credential: it files with the token the project's sessions get.
// Jira files through a Jira credential, and YouTrack through the person's own YouTrack account.
export const credentialKindByProvider = {
  jira: 'jira-credential',
  github: 'project-token',
  youtrack: 'youtrack-account',
} as const satisfies Record<LinkProvider, 'jira-credential' | 'project-token' | 'youtrack-account'>

// What a tracker's place is called on each provider, labelling the picker's last field.
export const destinationLabelKeys = {
  jira: 'manager.writes.destinations.jira',
  github: 'manager.writes.destinations.github',
  youtrack: 'manager.writes.destinations.youtrack',
} as const satisfies Record<LinkProvider, ParseKeys<'projects'>>

export const trackerSourceLabelKeys = {
  named: 'manager.writes.sources.named',
  precedent: 'manager.writes.sources.precedent',
} as const satisfies Record<TrackerSource, ParseKeys<'projects'>>

// What the tracker picker holds while a person chooses.
export type TrackerDraft = {
  provider: LinkProvider
  // The Jira credential or YouTrack account; empty until chosen, and always for GitHub.
  credentialId: string
  // A destination's reference; empty until chosen.
  destination: string
}

// The picker opens on the tracker the project has, or on Jira with nothing chosen.
export function trackerDraftFrom(tracker: ProjectTracker | null): TrackerDraft {
  if (!tracker) {
    return { provider: 'jira', credentialId: '', destination: '' }
  }
  return {
    provider: tracker.provider,
    credentialId: credentialKindByProvider[tracker.provider] === 'project-token'
      ? ''
      : tracker.credentialId,
    destination: tracker.destination,
  }
}

// The credential the destinations are read through: none for GitHub, which reads through the
// project's token; `null` while a provider that needs one has none chosen.
export function draftCredential(draft: TrackerDraft): { credentialId: string | null } | null {
  if (credentialKindByProvider[draft.provider] === 'project-token') {
    return { credentialId: null }
  }
  if (!draft.credentialId) {
    return null
  }
  return { credentialId: draft.credentialId }
}

// The request a finished draft saves, or `null` while something is still to choose.
export function toTrackerRequest(draft: TrackerDraft) {
  const credential = draftCredential(draft)
  if (!credential || !draft.destination) {
    return null
  }
  if (credential.credentialId === null) {
    return { provider: draft.provider, destination: draft.destination } as const
  }
  return {
    provider: draft.provider,
    credentialId: credential.credentialId,
    destination: draft.destination,
  } as const
}

// The YouTrack accounts `personId` owns, from their YouTrack connectors, once each. A tracker
// files through a person's own account alone.
export function youtrackAccountOptions(connectors: Connector[], personId: string | null): PickerOption[] {
  const options: PickerOption[] = []
  const seen = new Set<string>()
  for (const connector of connectors) {
    const account = connector.account
    if (!account || account.kind !== 'youtrack' || connector.createdBy !== personId) {
      continue
    }
    if (seen.has(account.id)) {
      continue
    }
    seen.add(account.id)
    options.push({ id: account.id, label: account.name })
  }
  return options
}
