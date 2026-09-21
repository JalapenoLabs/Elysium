// Copyright © 2026 Jalapeno Labs

import type { ActionItemPriority, LinkKind, LinkProvider } from './actionItemRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `ChangesetState` in api/src/models/changeset.rs.
export const CHANGESET_STATES = [ 'pending', 'applied', 'rejected', 'undone' ] as const
export type ChangesetState = typeof CHANGESET_STATES[number]

// The user's decision on one operation.
export const CHANGESET_DECISIONS = [ 'pending', 'approved', 'rejected' ] as const
export type ChangesetDecision = typeof CHANGESET_DECISIONS[number]

// What applying, and later undoing, did with one operation.
export const CHANGESET_OUTCOMES = [ 'pending', 'applied', 'failed', 'skipped', 'undone' ] as const
export type ChangesetOutcome = typeof CHANGESET_OUTCOMES[number]

// What an operation acts on: an item or initiative that exists, or the one an earlier
// operation of the same changeset creates, by its position.
export type OperationTarget = { id: string } | { operation: number }

// Mirrors `Operation` in api/src/action_items/changesets.rs.
export type ChangesetOperationChange =
  | {
    kind: 'create-item'
    title: string
    notes: string
    priority: ActionItemPriority
    dueAt: string | null
    projectIds: string[]
    initiatives: OperationTarget[]
  }
  // Only the fields it names change; `dueAt: null` removes the due date.
  | {
    kind: 'update-item'
    item: OperationTarget
    title?: string
    notes?: string
    priority?: ActionItemPriority
    dueAt?: string | null
  }
  | { kind: 'resolve-item', item: OperationTarget }
  | { kind: 'dismiss-item', item: OperationTarget }
  | { kind: 'comment', item: OperationTarget, body: string }
  | {
    kind: 'link'
    item: OperationTarget
    target: { provider: LinkProvider, credentialId: string, kind: LinkKind, reference: string }
  }
  | { kind: 'add-to-initiative', item: OperationTarget, initiative: OperationTarget }
  | { kind: 'remove-from-initiative', item: OperationTarget, initiative: OperationTarget }
  | {
    kind: 'create-initiative'
    name: string
    description: string
    targetAt: string | null
    projectIds: string[]
  }

export type ChangesetOperationKind = ChangesetOperationChange['kind']

// A provider and the key a person reads, such as `ELY-12`.
export type ProviderThing = {
  provider: LinkProvider
  key: string
}

// What undoing an operation did and could not do. Every field is optional; an empty object
// means it was reversed fully.
export type OperationUndo = {
  // Fields the user changed after the changeset, which kept the user's value.
  kept?: string[]
  // The item was moved since, so it stayed where the user put it.
  movedSince?: boolean
  // The comment had already been posted, and stays on the provider.
  stillPosted?: ProviderThing
  // Linked issues already closed, which stay closed.
  stillClosed?: ProviderThing[]
  // Why the records refused to reverse it, in the API's words.
  refusal?: string
}

// Mirrors `OperationResponse` in api/src/routes/v1/changesets/mod.rs.
export type ChangesetOperation = {
  id: string
  // From 1, in the order operations apply.
  position: number
  operation: ChangesetOperationChange
  reason: string
  quote: string | null
  source: string | null
  // The positions of the earlier operations whose records this one acts on.
  dependsOn: number[]
  decision: ChangesetDecision
  outcome: ChangesetOutcome
  // Why it failed or was skipped.
  error: string | null
  // The ids it acted on or created, and the values it replaced; shaped by its kind.
  result: Record<string, unknown>
  undo: OperationUndo | null
}

// Mirrors `ChangesetResponse` in api/src/routes/v1/changesets/mod.rs.
export type Changeset = {
  id: string
  // `elysia` or `session:<number>`.
  proposer: string
  projectId: string | null
  summary: string
  state: ChangesetState
  decidedAt: string | null
  undoneAt: string | null
  operations: ChangesetOperation[]
  createdAt: string
  updatedAt: string
}

type ListChangesetsResponse = {
  changesets: Changeset[]
}

// Newest first.
export function listChangesets() {
  return apiClient
    .get('v1/changesets')
    .json<ListChangesetsResponse>()
}

type ChangesetResponse = {
  changeset: Changeset
}

export function getChangeset(changesetId: string) {
  return apiClient
    .get(`v1/changesets/${changesetId}`)
    .json<ChangesetResponse>()
}

// Without `operationIds` the decision is for every operation. Rejecting an operation rejects
// everything that depends on it.
type DecideChangesetRequest = {
  decision: ChangesetDecision
  operationIds?: string[]
}

export function decideChangeset(changesetId: string, body: DecideChangesetRequest) {
  return apiClient
    .post(`v1/changesets/${changesetId}/decide`, { json: body })
    .json<ChangesetResponse>()
}

// Applies the approved operations; every operation must be decided first.
export function applyChangeset(changesetId: string) {
  return apiClient
    .post(`v1/changesets/${changesetId}/apply`)
    .json<ChangesetResponse>()
}

export function undoChangeset(changesetId: string) {
  return apiClient
    .post(`v1/changesets/${changesetId}/undo`)
    .json<ChangesetResponse>()
}
