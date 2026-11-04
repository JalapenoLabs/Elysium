// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { formatDuration, formatMediaPosition } from './durations'

describe('formatDuration', () => {
  it('writes under a second in milliseconds', () => {
    expect(formatDuration(850)).toBe('850ms')
  })

  it('writes under a minute in seconds, to one decimal', () => {
    expect(formatDuration(2_430)).toBe('2.4s')
    expect(formatDuration(42_000)).toBe('42s')
  })

  it('writes the two largest units from a minute up', () => {
    expect(formatDuration(184_000)).toBe('3m 4s')
    expect(formatDuration(2 * 3_600_000 + 5 * 60_000 + 59_000)).toBe('2h 5m')
  })

  it('counts a span that came out negative from zero', () => {
    expect(formatDuration(-1_200)).toBe('0ms')
  })
})

describe('formatMediaPosition', () => {
  it('writes minutes and padded seconds', () => {
    expect(formatMediaPosition(184_000)).toBe('3:04')
    expect(formatMediaPosition(0)).toBe('0:00')
  })

  it('adds hours from an hour on', () => {
    expect(formatMediaPosition(3_723_000)).toBe('1:02:03')
  })
})
