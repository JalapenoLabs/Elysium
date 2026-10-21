// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { isServerUrl } from './passwordsPresentation'

describe('isServerUrl', () => {
  it('accepts an https URL, with or without a path', () => {
    expect(isServerUrl('https://vault.example.com')).toBe(true)
    expect(isServerUrl(' https://example.com/vault ')).toBe(true)
  })

  it('refuses what the API refuses', () => {
    expect(isServerUrl('http://vault.example.com')).toBe(false)
    expect(isServerUrl('https://me:secret@vault.example.com')).toBe(false)
    expect(isServerUrl('https://vault.example.com/?next=1')).toBe(false)
    expect(isServerUrl('https://vault.example.com/#top')).toBe(false)
    expect(isServerUrl('vault.example.com')).toBe(false)
  })
})
