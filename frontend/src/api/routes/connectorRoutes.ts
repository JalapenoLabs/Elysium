// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `ConnectorKind` in api/src/connectors/mod.rs.
export const CONNECTOR_KINDS = [ 'outlook', 'gmail' ] as const
export type ConnectorKind = typeof CONNECTOR_KINDS[number]

export type SignInState = 'signed_in' | 'signed_out' | 'unknown'

// A service Elysium reads through one of the person's browsers. Only its owner receives it.
export type ConnectorAccount = {
  id: string
  kind: ConnectorKind
  browserSessionId: string
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

type CreateConnectorAccountRequest = {
  kind: ConnectorKind
  browserSessionId: string
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
