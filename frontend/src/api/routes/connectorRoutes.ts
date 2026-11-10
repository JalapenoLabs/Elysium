// Copyright © 2026 Jalapeno Labs

import type { ReplyPlace } from './draftRoutes'

// Misc
import { apiClient } from '../index'

// Mirrors `ConnectorKind` in api/src/connectors/mod.rs, in the order the hub lists them.
export const CONNECTOR_KINDS = [
  'jira',
  'confluence',
  'github',
  'outlook',
  'gmail',
  'teams',
  'slack',
  'fireflies',
  'fireflies_browser',
  'youtrack',
] as const
export type ConnectorKind = typeof CONNECTOR_KINDS[number]

// Mirrors `AccountKind` in api/src/connectors/mod.rs.
export const ACCOUNT_KINDS = [ 'atlassian', 'github', 'browser', 'fireflies', 'youtrack' ] as const
export type AccountKind = typeof ACCOUNT_KINDS[number]

// Mirrors `Provides` in api/src/connectors/kinds/mod.rs: the kinds of thing a connector reads.
export const CONNECTOR_PROVIDES = [ 'issues', 'pages', 'mail', 'messages', 'meetings' ] as const
export type ConnectorProvides = typeof CONNECTOR_PROVIDES[number]

export type SignInState = 'signed_in' | 'signed_out' | 'unknown'
export type ConnectorScope = 'workspace' | 'personal'
export type ConnectorStatus = 'active' | 'syncing' | 'error' | 'paused'
export type CheckState = 'queued' | 'running' | 'passed' | 'failed' | 'skipped'

// One check a kind runs, in order. The web app names it from `connectors:checks.<kind>.<id>`.
export type CheckSpec = {
  id: string
  required: boolean
  dependsOn: string[]
}

export type ContentField = {
  id: string
  // A selection picks from what the account reaches; a toggle is on or off, off unless saved.
  field: { type: 'selection' } | { type: 'storage_location' } | { type: 'toggle' }
  required: boolean
}

export type AccountFieldType =
  | { type: 'text' }
  | { type: 'email' }
  | { type: 'url' }
  | { type: 'secret' }
  | { type: 'choice', options: string[] }

export type AccountField = {
  id: string
  field: AccountFieldType
  required: boolean
}

// Mirrors `Replies` in api/src/connectors/kinds/mod.rs: how what a kind reads is answered as its
// person. `connector` sends through the connector itself, in any of `places`; `mailbox` sends
// mail from one of the person's own mailboxes.
export type ConnectorReplies =
  | { through: 'never' }
  | { through: 'connector', places: ReplyPlace[] }
  | { through: 'mailbox' }

// What the hub knows about a kind; structure only, every id named from translations.
export type ConnectorKindDefinition = {
  kind: ConnectorKind
  scope: ConnectorScope
  provides: ConnectorProvides[]
  // The account each of its connectors owns.
  account: AccountKind
  // The page its browser opens at, for a kind that reads through one.
  startUrl: string | null
  content: ContentField[]
  checks: CheckSpec[]
  replies: ConnectorReplies
}

export type AccountDefinition = {
  kind: AccountKind
  // Empty for a browser, which is signed in to on its stream.
  fields: AccountField[]
}

// What a check found. `count` fills its sentence, `reason` is why in Elysium's own terms,
// `message` is what the provider said, and `blockedBy` names the check it waited on.
export type CheckOutcome = {
  state: CheckState
  count: number | null
  reason: string | null
  message: string | null
  blockedBy: string | null
}

export type CheckResult = CheckOutcome & {
  checkId: string
  ranAt: string
}

export type AccountSummary = {
  id: string
  kind: AccountKind
  name: string
}

export type ConnectorRun = {
  id: string
  connectorId: string
  startedAt: string
  finishedAt: string
  outcome: 'succeeded' | 'failed'
  records: number
  error: string | null
}

// A connector as the hub shows it. Never carries a secret.
export type Connector = {
  id: string
  kind: ConnectorKind
  scope: ConnectorScope
  // The account it owns; null for one read through a browser while it waits for one.
  account: AccountSummary | null
  status: ConnectorStatus
  isActive: boolean
  signInState: SignInState
  lastSyncAt: string | null
  // Why the last read failed, or null when it succeeded.
  lastError: string | null
  // Timeline records, or linked issues for Jira and GitHub.
  records: number
  // What it reads, in its kind's own terms.
  content: Record<string, unknown>
  storageLocationId: string | null
  checks: CheckResult[]
  lastRun: ConnectorRun | null
  createdBy: string
  createdAt: string
}

export type CheckProgress = CheckOutcome & {
  checkId: string
  required: boolean
}

// A run of a connector's checks, or of one being set up (`connectorId` null).
export type CheckRun = {
  id: string
  kind: ConnectorKind
  connectorId: string | null
  startedAt: string
  finishedAt: string | null
  checks: CheckProgress[]
}

// The account a request names: a new one in its kind's own fields, for a connector being set
// up, or the account a stored connector owns, to list what its content can pick from.
export type AccountChoice =
  | { id: string }
  | { new: Record<string, unknown> }

export type ConnectorDraft = {
  kind: ConnectorKind
  account: AccountChoice
  content: Record<string, unknown>
}

// What a selection field can pick from.
// The provider's own words come as `label` and `detail`; Elysium's own come as `labelKey` and
// `detailKey`, keys under `fields.options`.
export type SelectionOption = {
  id: string | number
  label?: string
  labelKey?: string
  detail?: string | null
  detailKey?: string
}

export type SelectionOptions = {
  options: SelectionOption[]
  truncated: boolean
}

type ListConnectorKindsResponse = {
  kinds: ConnectorKindDefinition[]
  accounts: AccountDefinition[]
}

export function listConnectorKinds() {
  return apiClient
    .get('v1/connectors/kinds')
    .json<ListConnectorKindsResponse>()
}

type ListConnectorsResponse = {
  connectors: Connector[]
}

export function listConnectors() {
  return apiClient
    .get('v1/connectors')
    .json<ListConnectorsResponse>()
}

type ConnectorResponse = {
  connector: Connector
}

export function getConnector(connectorId: string) {
  return apiClient
    .get(`v1/connectors/${connectorId}`)
    .json<ConnectorResponse>()
}

export function createConnector(body: ConnectorDraft) {
  return apiClient
    .post('v1/connectors', { json: body })
    .json<ConnectorResponse>()
}

type UpdateConnectorRequest = {
  isActive?: boolean
  content?: Record<string, unknown>
}

export function updateConnector(connectorId: string, body: UpdateConnectorRequest) {
  return apiClient
    .patch(`v1/connectors/${connectorId}`, { json: body })
    .json<ConnectorResponse>()
}

export function deleteConnector(connectorId: string) {
  return apiClient.delete(`v1/connectors/${connectorId}`)
}

export function syncConnector(connectorId: string) {
  return apiClient.post(`v1/connectors/${connectorId}/sync`)
}

// Starts a browser for a connector read through one that is waiting for its browser.
export function signInConnector(connectorId: string) {
  return apiClient
    .post(`v1/connectors/${connectorId}/sign-in`)
    .json<ConnectorResponse>()
}

type CheckRunResponse = {
  run: CheckRun
}

export function runConnectorChecks(connectorId: string) {
  return apiClient
    .post(`v1/connectors/${connectorId}/checks`)
    .json<CheckRunResponse>()
}

export function checkConnectorDraft(body: ConnectorDraft) {
  return apiClient
    .post('v1/connectors/checks', { json: body })
    .json<CheckRunResponse>()
}

export function getCheckRun(runId: string) {
  return apiClient
    .get(`v1/connectors/check-runs/${runId}`)
    .json<CheckRunResponse>()
}

type ListOptionsResponse = {
  fields: Record<string, SelectionOptions>
}

export function listConnectorOptions(kind: ConnectorKind, account: AccountChoice) {
  return apiClient
    .post('v1/connectors/options', { json: { kind, account }})
    .json<ListOptionsResponse>()
}

type ListActivityResponse = {
  runs: ConnectorRun[]
}

export function listConnectorActivity() {
  return apiClient
    .get('v1/connectors/activity')
    .json<ListActivityResponse>()
}

