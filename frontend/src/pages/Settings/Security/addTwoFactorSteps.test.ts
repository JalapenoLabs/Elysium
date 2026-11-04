// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getAddTwoFactorStepUrl, readAddTwoFactorStep } from './addTwoFactorSteps'

describe('readAddTwoFactorStep', () => {
  it('starts at naming the app', () => {
    expect(readAddTwoFactorStep(new URLSearchParams())).toBe('name')
  })

  it('reads a later step once the app has a name', () => {
    expect(readAddTwoFactorStep(new URLSearchParams('step=codes&name=Bitwarden'))).toBe('codes')
    expect(readAddTwoFactorStep(new URLSearchParams('step=app&name=Bitwarden'))).toBe('app')
  })

  it('goes back to naming the app when the address has no name for it', () => {
    expect(readAddTwoFactorStep(new URLSearchParams('step=app'))).toBe('name')
    expect(readAddTwoFactorStep(new URLSearchParams('step=codes&name=%20'))).toBe('name')
  })

  it('reads an unknown step as the first', () => {
    expect(readAddTwoFactorStep(new URLSearchParams('step=finish&name=Bitwarden'))).toBe('name')
  })
})

describe('getAddTwoFactorStepUrl', () => {
  it('keeps the flows the page holds and sets the step', () => {
    const params = new URLSearchParams('step=codes&name=Bitwarden&codesFlow=abc')
    const url = new URL(getAddTwoFactorStepUrl(params, 'app'), 'http://localhost')
    expect(url.pathname).toBe('/settings/security/two-factor/new')
    expect(url.searchParams.get('step')).toBe('app')
    expect(url.searchParams.get('name')).toBe('Bitwarden')
    expect(url.searchParams.get('codesFlow')).toBe('abc')
  })

  it('replaces the name when given one', () => {
    const url = new URL(getAddTwoFactorStepUrl(new URLSearchParams(), 'codes', 'My phone'), 'http://localhost')
    expect(url.searchParams.get('name')).toBe('My phone')
  })
})
