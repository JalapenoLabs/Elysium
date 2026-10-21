// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `ConnectorKind` in api/src/connectors/mod.rs.
export const CONNECTOR_KINDS = [ 'outlook', 'gmail', 'fireflies' ] as const
export type ConnectorKind = typeof CONNECTOR_KINDS[number]

// The kinds that read mail through one of the person's browsers.
export const WEBMAIL_CONNECTOR_KINDS = [ 'outlook', 'gmail' ] as const satisfies readonly ConnectorKind[]
export type WebmailConnectorKind = typeof WEBMAIL_CONNECTOR_KINDS[number]

// The kinds that read meetings with an API key.
export const MEETING_CONNECTOR_KINDS = [ 'fireflies' ] as const satisfies readonly ConnectorKind[]
export type MeetingConnectorKind = typeof MEETING_CONNECTOR_KINDS[number]

export type SignInState = 'signed_in' | 'signed_out' | 'unknown'

// A service Elysium reads as the person, through one of their browsers or with an API key
// they gave it. Only its owner receives it, and a key is never sent back.
export type ConnectorAccount = {
  id: string
  kind: ConnectorKind
  // The browser it reads through, or null for an account that signs in with a key.
  browserSessionId: string | null
  hasCredential: boolean
  // Where it keeps files, such as meeting recordings.
  storageLocationId: string | null
  // As the last read found it.
  signInState: SignInState
  lastPolledAt: string | null
  // Why the last read failed, or null when it succeeded.
  lastError: string | null
  isActive: boolean
  createdBy: string
  createdAt: string
  updatedAt: string
}

type ListConnectorAccountsResponse = {
  connectors: ConnectorAccount[]
}

export function listConnectorAccounts() {
  return apiClient
    .get('v1/connectors')
    .json<ListConnectorAccountsResponse>()
}

// Each kind signs in its own way, and the API refuses a field the kind does not take.
type CreateConnectorAccountRequest =
  | {
    kind: WebmailConnectorKind
    browserSessionId: string
  }
  | {
    kind: MeetingConnectorKind
    apiKey: string
    storageLocationId: string
  }

type ConnectorAccountResponse = {
  connector: ConnectorAccount
}

export function createConnectorAccount(body: CreateConnectorAccountRequest) {
  return apiClient
    .post('v1/connectors', { json: body })
    .json<ConnectorAccountResponse>()
}

export function deleteConnectorAccount(connectorAccountId: string) {
  return apiClient.delete(`v1/connectors/${connectorAccountId}`)
}
