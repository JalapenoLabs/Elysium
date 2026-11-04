// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { checkPasswordRequirements, checkPasswordRules, isPasswordAcceptable } from './passwordRules'

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

describe('checkPasswordRequirements', () => {
  const allRules = { length: true, uppercase: true, special: true }

  it('lists strength beside the rules, met from a score of three', () => {
    expect(checkPasswordRequirements(allRules, 3)).toEqual({ ...allRules, strength: true })
    expect(checkPasswordRequirements(allRules, 2).strength).toBe(false)
  })

  it('leaves strength unmet until zxcvbn has judged the password', () => {
    expect(checkPasswordRequirements(allRules, null).strength).toBe(false)
  })
})

describe('isPasswordAcceptable', () => {
  const allMet = { length: true, uppercase: true, special: true, strength: true }

  it('needs every requirement, strength included', () => {
    expect(isPasswordAcceptable(allMet)).toBe(true)
    expect(isPasswordAcceptable({ ...allMet, strength: false })).toBe(false)
    expect(isPasswordAcceptable({ ...allMet, special: false })).toBe(false)
  })
})
