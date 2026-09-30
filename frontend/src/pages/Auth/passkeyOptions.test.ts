// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { readPasskeyCreationOptions, readPasskeyRequestOptions } from './passkeyOptions'

describe('readPasskeyRequestOptions', () => {
  it('unwraps the options Kratos puts in passkey_challenge', () => {
    const challenge = JSON.stringify({
      publicKey: { challenge: 'Y2hhbGxlbmdl', rpId: 'localhost', userVerification: 'preferred' },
    })
    expect(readPasskeyRequestOptions(challenge)).toEqual({
      challenge: 'Y2hhbGxlbmdl',
      rpId: 'localhost',
      userVerification: 'preferred',
    })
  })

  it('refuses a value without options rather than starting an empty ceremony', () => {
    expect(() => readPasskeyRequestOptions('{}')).toThrow()
    expect(() => readPasskeyRequestOptions('not json')).toThrow()
  })
})

describe('readPasskeyCreationOptions', () => {
  it('unwraps the options Kratos puts in passkey_create_data', () => {
    const created = JSON.stringify({ publicKey: { challenge: 'Yw', rp: { name: 'Elysium', id: 'localhost' }}})
    expect(readPasskeyCreationOptions(created).rp).toEqual({ name: 'Elysium', id: 'localhost' })
  })
})
