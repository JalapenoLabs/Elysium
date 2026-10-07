// Copyright © 2026 Jalapeno Labs

import type { OAuthClient } from '../../api/routes/oauthRoutes'

// Utility
import { HTTPError } from 'ky'

// The scopes a person can grant an MCP client, and how each reads. Any other scope a client asks
// for is never granted, so it is never shown.
export const scopeLabelKeys = {
  'workspace:read': 'scopes.read',
  'workspace:write': 'scopes.write',
  'offline_access': 'scopes.offline',
} as const satisfies Record<string, string>

export type KnownScope = keyof typeof scopeLabelKeys

export function isKnownScope(scope: string): scope is KnownScope {
  return Object.hasOwn(scopeLabelKeys, scope)
}

// The scopes worth showing, in the order the table above lists them.
export function knownScopes(scopes: string[]): KnownScope[] {
  const known: KnownScope[] = []
  for (const scope of Object.keys(scopeLabelKeys)) {
    if (isKnownScope(scope) && scopes.includes(scope)) {
      known.push(scope)
    }
  }
  return known
}

// What went wrong answering a challenge, as the page explains it.
export type ChallengeFailure = 'expired' | 'someoneElse' | 'unexpected'

export function challengeFailure(error: unknown): ChallengeFailure {
  if (!(error instanceof HTTPError)) {
    return 'unexpected'
  }
  if (error.response.status === 404) {
    return 'expired'
  }
  if (error.response.status === 403) {
    return 'someoneElse'
  }
  return 'unexpected'
}

// The host a client says it belongs to, when it gave a usable one.
export function clientHost(client: Pick<OAuthClient, 'uri'>): string | null {
  if (!client.uri) {
    return null
  }
  try {
    return new URL(client.uri).host || null
  }
  catch (error) {
    console.debug('clientHost could not read a client URI', { uri: client.uri, error })
    return null
  }
}
