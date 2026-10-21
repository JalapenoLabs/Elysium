// Copyright © 2026 Jalapeno Labs

import type { PasswordVault, VaultStatus } from '../api/routes/passwordVaultRoutes'

// Core
import { describe, expect, it } from 'vitest'

// Redux
import {
  passwordVaultDeleted,
  passwordVaultLoaded,
  passwordVaultSlice,
  passwordVaultStatusUpdated,
  passwordVaultUpdated,
} from './passwordVaultSlice'

const vault: PasswordVault = {
  server: 'self-hosted',
  serverUrl: 'https://vault.example.com',
  clientId: 'user.6c2b0d8e-2f9e-4c52-9a8e-0e3c1f6e8d11',
  accountEmail: 'elysia@example.com',
  collection: null,
  checkedAt: '2026-10-21T09:00:00Z',
  createdBy: '0199a3c4-0000-7000-8000-00000000beef',
  createdAt: '2026-10-21T09:00:00Z',
  updatedAt: '2026-10-21T09:00:00Z',
}

const ready: VaultStatus = { state: 'ready', error: null, lastSyncedAt: '2026-10-21T09:00:00Z' }

describe('passwordVaultSlice', () => {
  it('knows the vault is missing only once it has loaded', () => {
    const initial = passwordVaultSlice.reducer(undefined, { type: 'init' })
    expect(initial.loaded).toBe(false)

    const loaded = passwordVaultSlice.reducer(initial, passwordVaultLoaded({ vault: null, status: ready }))
    expect(loaded.loaded).toBe(true)
    expect(loaded.vault).toBeNull()
  })

  it('keeps the vault and its status apart', () => {
    let state = passwordVaultSlice.reducer(undefined, passwordVaultLoaded({ vault, status: ready }))
    state = passwordVaultSlice.reducer(
      state,
      passwordVaultStatusUpdated({ state: 'failed', error: 'Invalid master password.', lastSyncedAt: null }),
    )
    expect(state.vault).toEqual(vault)
    expect(state.status?.state).toBe('failed')

    const chosen = { ...vault, collection: { id: 'e1', name: 'Elysia', organizationId: 'o1' }}
    state = passwordVaultSlice.reducer(state, passwordVaultUpdated(chosen))
    expect(state.vault?.collection?.name).toBe('Elysia')
    expect(state.status?.state).toBe('failed')

    state = passwordVaultSlice.reducer(state, passwordVaultDeleted())
    expect(state.vault).toBeNull()
    expect(state.loaded).toBe(true)
  })
})
