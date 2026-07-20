// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { selectableTokenScopes, tokenScopeLabelKey } from './tokenScopes'
import { TokenScope } from '../proto/generated/security_pb'

describe('tokenScopes', () => {
  it('offers every real scope and never UNSPECIFIED in the mint form', () => {
    expect(selectableTokenScopes).toEqual([
      TokenScope.GIT_READ,
      TokenScope.GIT_WRITE,
      TokenScope.REGISTRY_PULL,
      TokenScope.REGISTRY_PUSH,
      TokenScope.MAIL_SEND,
    ])
    expect(selectableTokenScopes).not.toContain(TokenScope.UNSPECIFIED)
  })

  it('labels every enum value, so a new scope cannot render blank', () => {
    const numericScopes = Object.values(TokenScope).filter(
      (value): value is TokenScope => typeof value === 'number',
    )
    for (const scope of numericScopes) {
      expect(tokenScopeLabelKey[scope]).toBeTruthy()
    }
  })
})
