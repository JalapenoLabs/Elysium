// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getLoginMode, isSecondFactorRedirect } from './loginPresentation'

describe('getLoginMode', () => {
  it('asks for the second factor whenever the flow requires it', () => {
    expect(getLoginMode({ requested_aal: 'aal2', refresh: true })).toBe('secondFactor')
  })

  it('confirms a recent sign-in for a refresh flow', () => {
    expect(getLoginMode({ requested_aal: 'aal1', refresh: true })).toBe('refresh')
  })

  it('is a plain sign-in otherwise, and while the flow loads', () => {
    expect(getLoginMode({ requested_aal: 'aal1', refresh: false })).toBe('signIn')
    expect(getLoginMode(null)).toBe('signIn')
  })
})

describe('isSecondFactorRedirect', () => {
  it('recognizes Kratos asking for the second factor', () => {
    const url = 'http://localhost:4000/api/identity/self-service/login/browser?aal=aal2'
    expect(isSecondFactorRedirect(url)).toBe(true)
  })

  it('leaves every other redirect alone', () => {
    expect(isSecondFactorRedirect('http://localhost:4000/settings/security?flow=1')).toBe(false)
    expect(isSecondFactorRedirect('http://localhost:4000/api/identity/self-service/login/browser')).toBe(false)
  })
})
