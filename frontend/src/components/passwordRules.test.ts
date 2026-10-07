// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { checkPasswordRules, isPasswordAcceptable } from './passwordRules'

describe('checkPasswordRules', () => {
  it('meets every rule with twelve characters, an uppercase letter, and a special one', () => {
    expect(checkPasswordRules('Correct-horse-9')).toEqual({ length: true, uppercase: true, special: true })
  })

  it('reports each rule a password misses', () => {
    expect(checkPasswordRules('short')).toEqual({ length: false, uppercase: false, special: false })
    expect(checkPasswordRules('lowercase only here')).toEqual({ length: true, uppercase: false, special: false })
    expect(checkPasswordRules('Twelve chars ok')).toEqual({ length: true, uppercase: true, special: false })
  })

  it('counts characters, not UTF-16 units, as Kratos does', () => {
    // Eleven code points, twelve UTF-16 units.
    expect(checkPasswordRules('Abcdefghij😀').length).toBe(false)
    expect(checkPasswordRules('Abcdefghijk😀').length).toBe(true)
  })

  it('treats letters beyond English as letters, not special characters', () => {
    expect(checkPasswordRules('Ångström élan').special).toBe(false)
    expect(checkPasswordRules('Ängström-élan').uppercase).toBe(true)
  })
})

describe('isPasswordAcceptable', () => {
  const allRules = { length: true, uppercase: true, special: true }

  it('needs every rule and a score of at least three', () => {
    expect(isPasswordAcceptable(allRules, 3)).toBe(true)
    expect(isPasswordAcceptable(allRules, 4)).toBe(true)
    expect(isPasswordAcceptable(allRules, 2)).toBe(false)
    expect(isPasswordAcceptable({ ...allRules, special: false }, 4)).toBe(false)
  })

  it('waits for the strength estimate before accepting anything', () => {
    expect(isPasswordAcceptable(allRules, null)).toBe(false)
  })
})
