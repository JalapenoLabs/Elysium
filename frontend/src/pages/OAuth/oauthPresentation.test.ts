// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Utility
import ky from 'ky'

// Misc
import { challengeFailure, clientHost, isRedirect, knownScopes } from './oauthPresentation'

// The error ky itself throws for a response with this status.
function httpError(status: number) {
  return ky('http://localhost/api/v1/oauth/consent', {
    retry: 0,
    fetch: () => Promise.resolve(new Response(null, { status })),
  }).catch((error: unknown) => error)
}

describe('knownScopes', () => {
  it('keeps the grantable scopes in a stable order and drops the rest', () => {
    expect(knownScopes([ 'offline_access', 'openid', 'workspace:write', 'workspace:read' ]))
      .toEqual([ 'workspace:read', 'workspace:write', 'offline_access' ])
    expect(knownScopes([])).toEqual([])
  })
})

describe('challengeFailure', () => {
  it('reads an unknown or used challenge as expired, and another person\'s as theirs', async () => {
    expect(challengeFailure(await httpError(404))).toBe('expired')
    expect(challengeFailure(await httpError(403))).toBe('someoneElse')
    expect(challengeFailure(await httpError(502))).toBe('unexpected')
    expect(challengeFailure(new Error('offline'))).toBe('unexpected')
  })
})

describe('clientHost', () => {
  it('shows the host of a client\'s own address, and nothing for a missing or broken one', () => {
    expect(clientHost({ uri: 'https://claude.ai/code' })).toBe('claude.ai')
    expect(clientHost({ uri: '' })).toBeNull()
    expect(clientHost({ uri: 'not a url' })).toBeNull()
  })
})

describe('isRedirect', () => {
  it('tells an answer that already connected the client from one that asks', () => {
    expect(isRedirect({ redirectTo: 'http://localhost/oauth2/auth' })).toBe(true)
    expect(isRedirect({
      client: { id: 'a', name: 'Claude Code', uri: '', logoUri: '', createdAt: null },
      scopes: [ 'workspace:read' ],
    })).toBe(false)
  })
})
