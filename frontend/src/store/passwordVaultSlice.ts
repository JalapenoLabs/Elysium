// Copyright © 2026 Jalapeno Labs

import type { PayloadAction } from '@reduxjs/toolkit'
import type { PasswordVault, VaultStatus } from '../api/routes/passwordVaultRoutes'
import type { RootState } from './index'

// Core
import { createSlice } from '@reduxjs/toolkit'

type PasswordVaultSliceState = {
  // Whether an admin has loaded it yet; until then `vault` says nothing.
  loaded: boolean
  vault: PasswordVault | null
  status: VaultStatus | null
}

const initialState: PasswordVaultSliceState = {
  loaded: false,
  vault: null,
  status: null,
}

// The password vault agents sign in to things with, and what its CLI is doing. The vault and
// its status change separately: an admin saves the vault, while the CLI's status moves on its
// own as it signs in, syncs, or fails.
export const passwordVaultSlice = createSlice({
  name: 'passwordVault',
  initialState,
  reducers: {
    passwordVaultLoaded(state, action: PayloadAction<{ vault: PasswordVault | null, status: VaultStatus }>) {
      state.loaded = true
      state.vault = action.payload.vault
      state.status = action.payload.status
    },
    passwordVaultUpdated(state, action: PayloadAction<PasswordVault>) {
      state.vault = action.payload
    },
    passwordVaultDeleted(state) {
      state.vault = null
    },
    passwordVaultStatusUpdated(state, action: PayloadAction<VaultStatus>) {
      state.status = action.payload
    },
  },
})

export const {
  passwordVaultLoaded,
  passwordVaultUpdated,
  passwordVaultDeleted,
  passwordVaultStatusUpdated,
} = passwordVaultSlice.actions

export function selectPasswordVault(state: RootState) {
  return state.passwordVault.vault
}

export function selectPasswordVaultStatus(state: RootState) {
  return state.passwordVault.status
}

export function selectPasswordVaultLoaded(state: RootState) {
  return state.passwordVault.loaded
}
