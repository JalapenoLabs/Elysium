// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `ConnectorKind` in api/src/connectors/mod.rs, in the order the hub lists them.
export const CONNECTOR_KINDS = [ 'jira', 'github', 'outlook', 'gmail', 'fireflies', 'slack' ] as const
export type ConnectorKind = typeof CONNECTOR_KINDS[number]

// The kinds read through one of the person's browsers. Mirrors `ConnectorKind::BROWSER_READ`.
export const BROWSER_CONNECTOR_KINDS = [ 'outlook', 'gmail', 'slack' ] as const satisfies readonly ConnectorKind[]
export type BrowserConnectorKind = typeof BROWSER_CONNECTOR_KINDS[number]

// Mirrors `AccountKind` in api/src/connectors/mod.rs.
export const ACCOUNT_KINDS = [ 'atlassian', 'github', 'browser', 'fireflies' ] as const
export type AccountKind = typeof ACCOUNT_KINDS[number]

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
  field: { type: 'selection' } | { type: 'storage_location' }
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

// What the hub knows about a kind; structure only, every id named from translations.
export type ConnectorKindDefinition = {
  kind: ConnectorKind
  scope: ConnectorScope
  account: AccountKind
  // The page a new browser opens at, for a kind that reads through one.
  startUrl: string | null
  content: ContentField[]
  checks: CheckSpec[]
}

export type AccountDefinition = {
  kind: AccountKind
  // Empty for a browser, which is signed in to on its own page.
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

export type AccountUse = {
  connectorId: string
  kind: ConnectorKind
}

// An account as the hub lists it. Never carries a secret.
export type Account = {
  id: string
  kind: AccountKind
  name: string
  // Who the provider says it is, such as a login or an address.
  identity: string
  checkedAt: string | null
  // The workspace default token coding sessions start with, for a GitHub account.
  isDefault: boolean
  createdBy: string
  createdAt: string
  usedBy: AccountUse[]
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

// The account a connector being set up reads through: one the person has, or a new one in
// its kind's own fields.
export type AccountChoice =
  | { id: string }
  | { new: Record<string, unknown> }

export type ConnectorDraft = {
  kind: ConnectorKind
  account: AccountChoice
  content: Record<string, unknown>
}

// What a selection field can pick from. The provider's own words come as `label` and `detail`;
// Elysium's own come as `labelKey` and `detailKey`, keys under `fields.options`.
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

type ListAccountsResponse = {
  accounts: Account[]
}

export function listAccounts() {
  return apiClient
    .get('v1/accounts')
    .json<ListAccountsResponse>()
}

export function deleteTokenAccount(accountId: string) {
  return apiClient.delete(`v1/accounts/tokens/${accountId}`)
}
