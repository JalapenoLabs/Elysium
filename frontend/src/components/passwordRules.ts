// Copyright © 2026 Jalapeno Labs

// Misc
import { PASSWORD_MIN_LENGTH, PASSWORD_MIN_SCORE } from '../constants'

// The rules a new password must meet before a form submits it. Kratos enforces the length
// and refuses breached passwords, but cannot check character classes, so those two live here
// alone. See docs/auth.md.
export const PASSWORD_RULES = [ 'length', 'uppercase', 'special' ] as const
export type PasswordRule = typeof PASSWORD_RULES[number]

// Any script's uppercase letter counts.
const UPPERCASE_LETTER = /\p{Lu}/u

// Anything that is not a letter, a digit, or whitespace counts as special.
const SPECIAL_CHARACTER = /[^\p{L}\p{N}\s]/u

// Whether `password` meets each rule, checked in one pass.
export function checkPasswordRules(password: string): Record<PasswordRule, boolean> {
  // Counted by code point, as Kratos counts, so an emoji is one character, not two.
  return {
    length: [ ...password ].length >= PASSWORD_MIN_LENGTH,
    uppercase: UPPERCASE_LETTER.test(password),
    special: SPECIAL_CHARACTER.test(password),
  }
}

// A password the form may submit: every rule met, and zxcvbn thinks it strong enough.
export function isPasswordAcceptable(rules: Record<PasswordRule, boolean>, score: number | null) {
  const meetsRules = PASSWORD_RULES.every((rule) => rules[rule])
  return meetsRules && score !== null && score >= PASSWORD_MIN_SCORE
}
