// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'
import type { PasswordServer, VaultState } from '../../../api/routes/passwordVaultRoutes'

type ChipColor = 'default' | 'accent' | 'success' | 'warning' | 'danger'

export const serverLabelKeys = {
  'bitwarden-us': 'servers.bitwarden-us',
  'bitwarden-eu': 'servers.bitwarden-eu',
  'self-hosted': 'servers.self-hosted',
} as const satisfies Record<PasswordServer, ParseKeys<'passwords'>>

export const stateLabelKeys = {
  unconfigured: 'state.unconfigured',
  starting: 'state.starting',
  ready: 'state.ready',
  failed: 'state.failed',
} as const satisfies Record<VaultState, ParseKeys<'passwords'>>

export const stateChipColors = {
  unconfigured: 'default',
  starting: 'accent',
  ready: 'success',
  failed: 'danger',
} as const satisfies Record<VaultState, ChipColor>

export const setupStepKeys = [
  'setup.collection',
  'setup.account',
  'setup.apiKey',
  'setup.masterPassword',
] as const satisfies readonly ParseKeys<'passwords'>[]

// Whether `value` is a self-hosted server URL the API accepts: https, with a host, and no
// credentials, query, or fragment. Mirrors `validate_server` in
// api/src/routes/v1/password_vault/connect_password_vault.rs.
export function isServerUrl(value: string) {
  let url: URL
  try {
    url = new URL(value.trim())
  }
  catch {
    return false
  }
  return url.protocol === 'https:'
    && Boolean(url.hostname)
    && !url.username
    && !url.password
    && !url.search
    && !url.hash
}
