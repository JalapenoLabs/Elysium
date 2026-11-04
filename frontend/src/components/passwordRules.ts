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

// What the checklist under a new password lists: the rules, then strong enough by zxcvbn's
// score, so every condition that holds the form back is one the person can see.
export const PASSWORD_REQUIREMENTS = [ ...PASSWORD_RULES, 'strength' ] as const
export type PasswordRequirement = typeof PASSWORD_REQUIREMENTS[number]

// Whether each requirement is met. The score is null until zxcvbn has judged the password, which
// leaves strength unmet rather than guessed.
export function checkPasswordRequirements(
  rules: Record<PasswordRule, boolean>,
  score: number | null,
): Record<PasswordRequirement, boolean> {
  return {
    ...rules,
    strength: score !== null && score >= PASSWORD_MIN_SCORE,
  }
}

// A password the form may submit: every requirement met.
export function isPasswordAcceptable(requirements: Record<PasswordRequirement, boolean>) {
  return PASSWORD_REQUIREMENTS.every((requirement) => requirements[requirement])
}
