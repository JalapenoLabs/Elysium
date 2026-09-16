// Copyright © 2026 Jalapeno Labs

import type { ParseKeys } from 'i18next'

// Mirrors the API's limits so most mistakes are caught before a round trip.
export const KEY_MAX_CHARACTERS = 128
export const VALUE_MAX_BYTES = 32 * 1024
export const DESCRIPTION_MAX_CHARACTERS = 500

// Shown in place of a secret's value, which the API never returns.
export const MASKED_VALUE = '••••••'

// Why a key cannot be stored. Mirrors `KeyRefusal` in api/src/environment/mod.rs.
export type KeyRefusal =
  | 'malformed'
  | 'reservedForElysium'
  | 'reservedForSatellite'
  | 'providerCredential'
  | 'overriddenBySatellite'

export const keyRefusalMessageKeys = {
  malformed: 'form.errors.keyRefusal.malformed',
  reservedForElysium: 'form.errors.keyRefusal.reservedForElysium',
  reservedForSatellite: 'form.errors.keyRefusal.reservedForSatellite',
  providerCredential: 'form.errors.keyRefusal.providerCredential',
  overriddenBySatellite: 'form.errors.keyRefusal.overriddenBySatellite',
} as const satisfies Record<KeyRefusal, ParseKeys<'environment'>>

type KeyRule = {
  matches: (key: string) => boolean
  refusal: KeyRefusal
}

// Mirrors `is_provider_credential` in the satellite, by way of api/src/environment/mod.rs:
// a vendor's prefix and a word that marks a credential, matched on shape so a key neither
// side has listed is still caught.
function isProviderCredential(key: string) {
  const vendors = [ 'ANTHROPIC_', 'OPENAI_', 'AWS_', 'AZURE_', 'GOOGLE_', 'DEEPSEEK_' ]
  const markers = [ 'API_KEY', 'AUTH_TOKEN', 'ACCESS_KEY', 'SECRET', 'CREDENTIALS' ]

  return vendors.some((vendor) => key.startsWith(vendor))
    && markers.some((marker) => key.includes(marker))
}

// Mirrors `REFUSED_KEYS` in api/src/environment/mod.rs, which explains each rule. Keys match
// with their exact case, as processes see them.
const REFUSED_KEYS: readonly KeyRule[] = [
  // Elysium sets these for GitHub. The prefix covers GIT_CONFIG_PARAMETERS too.
  { matches: (key) => key === 'GH_TOKEN', refusal: 'reservedForElysium' },
  { matches: (key) => key === 'GITHUB_TOKEN', refusal: 'reservedForElysium' },
  { matches: (key) => key.startsWith('GIT_CONFIG_'), refusal: 'reservedForElysium' },
  // The satellite refuses these when a thread is created.
  { matches: (key) => key.startsWith('ARSOX_'), refusal: 'reservedForSatellite' },
  { matches: isProviderCredential, refusal: 'providerCredential' },
  // The satellite sets these after every variable a thread declares.
  { matches: (key) => key === 'PATH', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'HTTP_PROXY', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'http_proxy', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'HTTPS_PROXY', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'https_proxy', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'NO_PROXY', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'no_proxy', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'ANTHROPIC_BASE_URL', refusal: 'overriddenBySatellite' },
  { matches: (key) => key === 'OPENAI_BASE_URL', refusal: 'overriddenBySatellite' },
]

// Why `key` cannot be stored, or null when it can. Length is checked by the caller, so its
// message can say so.
export function getKeyRefusal(key: string): KeyRefusal | null {
  if (!/^[A-Za-z_][A-Za-z0-9_]*$/.test(key)) {
    return 'malformed'
  }

  const rule = REFUSED_KEYS.find((candidate) => candidate.matches(key))
  return rule?.refusal ?? null
}
