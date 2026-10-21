// Copyright © 2026 Jalapeno Labs

// Misc
import { apiClient } from '../index'

// Mirrors `PasswordServer` in api/src/models/password_vault.rs.
export const PASSWORD_SERVERS = [ 'bitwarden-us', 'bitwarden-eu', 'self-hosted' ] as const
export type PasswordServer = typeof PASSWORD_SERVERS[number]

// Mirrors `VaultState` in api/src/passwords/mod.rs.
export type VaultState = 'unconfigured' | 'starting' | 'ready' | 'failed'

// Mirrors `VaultStatus` in api/src/passwords/mod.rs: what the API's Bitwarden CLI is doing.
export type VaultStatus = {
  state: VaultState
  error: string | null
  lastSyncedAt: string | null
}

// Mirrors `Collection` in api/src/passwords/items.rs.
export type VaultCollection = {
  id: string
  organizationId: string
  name: string
}

// Mirrors `PasswordVaultResponse` in api/src/routes/v1/password_vault/mod.rs. The client
// secret and master password never leave the API.
export type PasswordVault = {
  server: PasswordServer
  // A self-hosted server's URL; null for Bitwarden's own servers.
  serverUrl: string | null
  clientId: string
  // The account the API key signs in as, as the CLI reported it.
  accountEmail: string
  // The one collection agents may read; null until an admin chooses it.
  collection: {
    id: string
    name: string
    organizationId: string
  } | null
  checkedAt: string
  createdBy: string
  createdAt: string
  updatedAt: string
}

type VaultResponse = {
  vault: PasswordVault | null
  status: VaultStatus
}

export function getPasswordVault() {
  return apiClient
    .get('v1/password-vault')
    .json<VaultResponse>()
}

type ConnectPasswordVaultRequest = {
  server: PasswordServer
  serverUrl?: string
  clientId: string
  // Left out to keep the stored one.
  clientSecret?: string
  // Left out to keep the stored one.
  masterPassword?: string
}

type ConnectPasswordVaultResponse = {
  vault: PasswordVault
  status: VaultStatus
  collections: VaultCollection[]
}

// Signs the API's CLI in and unlocks it before anything is stored, so a refused key or
// master password answers 400 with Bitwarden's own words.
export function connectPasswordVault(body: ConnectPasswordVaultRequest) {
  return apiClient
    .put('v1/password-vault', { json: body, timeout: 180_000 })
    .json<ConnectPasswordVaultResponse>()
}

type ListCollectionsResponse = {
  collections: VaultCollection[]
}

export function listVaultCollections() {
  return apiClient
    .get('v1/password-vault/collections')
    .json<ListCollectionsResponse>()
}

// Chooses the one collection agents may read, or clears it with null.
export function chooseVaultCollection(collectionId: string | null) {
  return apiClient
    .put('v1/password-vault/collection', { json: { collectionId }})
    .json<VaultResponse>()
}

type SyncResponse = {
  status: VaultStatus
}

export function syncPasswordVault() {
  return apiClient
    .post('v1/password-vault/sync', { timeout: 180_000 })
    .json<SyncResponse>()
}

export function deletePasswordVault() {
  return apiClient
    .delete('v1/password-vault')
}
