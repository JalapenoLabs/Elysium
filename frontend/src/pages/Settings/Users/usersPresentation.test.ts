// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { makeUser } from '../../../testFixtures'
import { describeSignInMethods, hasAuthenticator } from './usersPresentation'

describe('describeSignInMethods', () => {
  it('names each method Kratos reports', () => {
    const described = describeSignInMethods([ 'password', 'passkey', 'totp', 'lookup_secret' ])
    expect(described.map((entry) => entry.key)).toEqual([
      'methods.password',
      'methods.passkey',
      'methods.totp',
      'methods.lookupSecret',
    ])
  })

  it('keeps an unknown method as Kratos names it, and says nothing for none', () => {
    expect(describeSignInMethods([ 'oidc' ])).toEqual([{ method: 'oidc', key: null }])
    expect(describeSignInMethods(undefined)).toEqual([])
  })
})

describe('hasAuthenticator', () => {
  it('is true only with an authenticator app', () => {
    expect(hasAuthenticator(makeUser({ signInMethods: [ 'password', 'totp' ]}))).toBe(true)
    expect(hasAuthenticator(makeUser({ signInMethods: [ 'password', 'passkey' ]}))).toBe(false)
    expect(hasAuthenticator(makeUser({ signInMethods: undefined }))).toBe(false)
  })
})
