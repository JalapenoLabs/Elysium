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
  | 'reservedForRunner'
  | 'modelCredential'

export const keyRefusalMessageKeys = {
  malformed: 'form.errors.keyRefusal.malformed',
  reservedForElysium: 'form.errors.keyRefusal.reservedForElysium',
  reservedForRunner: 'form.errors.keyRefusal.reservedForRunner',
  modelCredential: 'form.errors.keyRefusal.modelCredential',
} as const satisfies Record<KeyRefusal, ParseKeys<'environment'>>

type KeyRule = {
  matches: (key: string) => boolean
  refusal: KeyRefusal
}

// Mirrors `REFUSED_KEYS` in api/src/environment/mod.rs, which explains each rule. Keys match
// with their exact case, as processes see them.
const REFUSED_KEYS: readonly KeyRule[] = [
  // Elysium sets these for GitHub. The prefix covers GIT_CONFIG_PARAMETERS too.
  { matches: (key) => key === 'GH_TOKEN', refusal: 'reservedForElysium' },
  { matches: (key) => key === 'GITHUB_TOKEN', refusal: 'reservedForElysium' },
  { matches: (key) => key.startsWith('GIT_CONFIG_'), refusal: 'reservedForElysium' },
  // The runner sets these for every turn.
  { matches: (key) => key === 'CLAUDE_CONFIG_DIR', refusal: 'reservedForRunner' },
  { matches: (key) => key.startsWith('ELYSIUM_'), refusal: 'reservedForRunner' },
  // These would decide the model credential or endpoint a turn runs on.
  { matches: (key) => key === 'CLAUDE_CODE_OAUTH_TOKEN', refusal: 'modelCredential' },
  { matches: (key) => key === 'ANTHROPIC_API_KEY', refusal: 'modelCredential' },
  { matches: (key) => key === 'ANTHROPIC_AUTH_TOKEN', refusal: 'modelCredential' },
  { matches: (key) => key === 'ANTHROPIC_BASE_URL', refusal: 'modelCredential' },
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
