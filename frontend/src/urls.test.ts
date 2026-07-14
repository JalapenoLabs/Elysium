// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, expectTypeOf, it } from 'vitest'

// Misc
import { getInviteUrl, UrlTree } from './urls'

describe('getInviteUrl', () => {
  it('substitutes the token into the invite route', () => {
    expect(getInviteUrl('abc123')).toBe('/invite/abc123')
  })

  it('leaves no :token placeholder behind', () => {
    const inviteUrl = getInviteUrl('signed-token-value')

    expect(inviteUrl).not.toContain(':token')
    expect(inviteUrl).toContain('signed-token-value')
  })

  it('builds URLs that stay under the invite route prefix', () => {
    const routePrefix = UrlTree.invite.replace(':token', '')

    expect(getInviteUrl('a-b_c.d~e').startsWith(routePrefix)).toBe(true)
  })

  it('preserves URL-safe token characters exactly', () => {
    // Signed tokens are URL-safe (base64url style); the factory must not mangle them.
    const token = 'eyJhbGciOiJIUzI1NiJ9.payload-part_x'

    expect(getInviteUrl(token)).toBe(`/invite/${token}`)
  })

  it('returns a string type', () => {
    const inviteUrl = getInviteUrl('typed-token')

    expectTypeOf(inviteUrl).toEqualTypeOf<string>()
    inviteUrl satisfies string
  })
})
