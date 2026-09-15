// Copyright © 2026 Jalapeno Labs

// Utility
import { HTTPError } from 'ky'

// The API answers 502 when an upstream (a satellite, a mail server, the OAuth broker)
// refuses or cannot be reached, with the upstream's own message. That message names what
// went wrong (an unreachable host, a rejected secret, a revoked grant), so it is worth
// showing; it never holds a secret.
export function getUpstreamErrorMessage(error: unknown): string | null {
  if (!(error instanceof HTTPError) || error.response.status !== 502) {
    return null
  }

  const data = error.data
  if (typeof data === 'object' && data !== null && 'message' in data && typeof data.message === 'string') {
    return data.message
  }

  console.debug('A 502 response carried no message', { data })
  return null
}
