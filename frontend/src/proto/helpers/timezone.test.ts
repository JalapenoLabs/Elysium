// Copyright © 2026 Jalapeno Labs

// Core
import { describe, it, expect } from 'vitest'

// Misc
import { buildTimezoneOptions, resolveDisplayTimezone } from './timezone'

describe('resolveDisplayTimezone', () => {
  it('returns a non-empty saved preference verbatim', () => {
    expect(resolveDisplayTimezone('America/Denver')).toBe('America/Denver')
  })

  it('treats a blank or whitespace-only preference as auto-detect', () => {
    // Auto-detect resolves to whatever zone the environment reports, so assert
    // only that it falls back to a real, non-empty zone rather than the input.
    expect(resolveDisplayTimezone('   ')).not.toBe('   ')
    expect(resolveDisplayTimezone('   ').length).toBeGreaterThan(0)
  })

  it('auto-detects a real zone when no preference is given', () => {
    expect(resolveDisplayTimezone().length).toBeGreaterThan(0)
    expect(resolveDisplayTimezone(undefined).length).toBeGreaterThan(0)
  })
})

describe('buildTimezoneOptions', () => {
  it('offers the full IANA zone list', () => {
    const options = buildTimezoneOptions()
    expect(options.length).toBeGreaterThan(100)
    expect(options.map((option) => option.zone)).toContain('America/Denver')
  })

  it('labels each zone with its UTC offset', () => {
    const denver = buildTimezoneOptions().find((option) => option.zone === 'America/Denver')
    // The offset shifts with DST, so assert the shape rather than a fixed hour.
    expect(denver?.label).toMatch(/^America\/Denver \(GMT[+-]\d{2}:\d{2}\)$/)
  })

  it('keeps the raw zone as the saved value, never the decorated label', () => {
    // Zones legitimately named after GMT exist (Etc/GMT+1, GMT0), so the tell
    // for a leaked label is the offset suffix, not the letters "GMT".
    const options = buildTimezoneOptions()
    expect(options.every((option) => !option.zone.includes('('))).toBe(true)
    expect(options.every((option) => option.zone !== option.label)).toBe(true)
  })
})
