// Copyright © 2026 Jalapeno Labs

// Utility
import { HTTPError } from 'ky'

// Every error the API answers is JSON with a `message` that names what went wrong and
// never holds a secret, whoever's fault it was.
export function getApiErrorMessage(error: unknown): string | null {
  if (!(error instanceof HTTPError)) {
    return null
  }

  const data = error.data
  if (typeof data === 'object' && data !== null && 'message' in data && typeof data.message === 'string') {
    return data.message
  }

  console.debug('An error response carried no message', { data })
  return null
}

// The API answers 502 when an upstream (a satellite, a mail server, the OAuth broker)
// refuses or cannot be reached, with the upstream's own message. That message names what
// went wrong (an unreachable host, a rejected secret, a revoked grant), so it is worth
// showing.
export function getUpstreamErrorMessage(error: unknown): string | null {
  if (!(error instanceof HTTPError) || error.response.status !== 502) {
    return null
  }

  return getApiErrorMessage(error)
}
