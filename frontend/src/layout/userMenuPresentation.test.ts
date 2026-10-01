// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { getInitials } from './userMenuPresentation'

describe('getInitials', () => {
  it('takes the first letters of the first and last words', () => {
    expect(getInitials('Ada King Lovelace')).toBe('AL')
    expect(getInitials('grace')).toBe('G')
  })

  it('keeps letters beyond ASCII whole', () => {
    expect(getInitials('Ólafur Élan')).toBe('ÓÉ')
  })

  it('shows a question mark for a blank name', () => {
    expect(getInitials('   ')).toBe('?')
  })
})
