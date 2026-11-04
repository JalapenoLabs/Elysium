// Copyright © 2026 Jalapeno Labs

// Core
import { afterEach, describe, expect, it, vi } from 'vitest'

// Misc
import {
  formatCalendarDay,
  formatDate,
  formatDateTime,
  formatRelativeTime,
  formatShortDate,
  formatShortDateTime,
  formatTime,
  formatWeekday,
} from './dates'

// 8:56pm on January 1st 2026 in Chicago, which is already January 2nd in UTC.
const NEW_YEAR_EVENING = '2026-01-02T02:56:00Z'
const CHICAGO = 'America/Chicago'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('formatDate', () => {
  it('writes the month by name, the day as an ordinal, and the year', () => {
    expect(formatDate(NEW_YEAR_EVENING, { timeZone: CHICAGO })).toBe('January 1st 2026')
  })

  it('shows the day in the viewer\'s zone, not the server\'s', () => {
    expect(formatDate(NEW_YEAR_EVENING, { timeZone: 'UTC' })).toBe('January 2nd 2026')
  })

  it('gives every day of the month its English ordinal', () => {
    const ordinals = [ 1, 2, 3, 4, 11, 12, 13, 21, 22, 23, 31 ].map((day) => {
      const isoDay = `2026-01-${String(day).padStart(2, '0')}T12:00:00Z`
      return formatDate(isoDay, { timeZone: 'UTC' })
    })
    expect(ordinals).toEqual([
      'January 1st 2026',
      'January 2nd 2026',
      'January 3rd 2026',
      'January 4th 2026',
      'January 11th 2026',
      'January 12th 2026',
      'January 13th 2026',
      'January 21st 2026',
      'January 22nd 2026',
      'January 23rd 2026',
      'January 31st 2026',
    ])
  })

  it('takes milliseconds and Date objects as well as ISO strings', () => {
    const instant = Date.parse(NEW_YEAR_EVENING)
    expect(formatDate(instant, { timeZone: CHICAGO })).toBe('January 1st 2026')
    expect(formatDate(new Date(instant), { timeZone: CHICAGO })).toBe('January 1st 2026')
  })

  it('warns and shows a value that is not a date as it is', () => {
    const warn = vi.spyOn(console, 'warn').mockImplementation(() => {})
    expect(formatDate('not a date')).toBe('not a date')
    expect(warn).toHaveBeenCalledOnce()
  })
})

describe('formatTime', () => {
  it('writes the hour and minute with a lowercase day period and no space', () => {
    expect(formatTime(NEW_YEAR_EVENING, { timeZone: CHICAGO })).toBe('8:56pm')
  })

  it('writes midnight and noon as twelve', () => {
    expect(formatTime('2026-01-01T00:05:00Z', { timeZone: 'UTC' })).toBe('12:05am')
    expect(formatTime('2026-01-01T12:00:00Z', { timeZone: 'UTC' })).toBe('12:00pm')
  })
})

describe('formatDateTime', () => {
  it('joins the date and the time with "at"', () => {
    expect(formatDateTime(NEW_YEAR_EVENING, { timeZone: CHICAGO })).toBe('January 1st 2026 at 8:56pm')
  })
})

describe('formatShortDate', () => {
  it('drops the year of a date in this year', () => {
    const now = Date.parse('2026-06-15T12:00:00Z')
    expect(formatShortDate(NEW_YEAR_EVENING, { now, timeZone: CHICAGO })).toBe('January 1st')
  })

  it('keeps the year of a date in another year', () => {
    const now = Date.parse('2027-02-01T12:00:00Z')
    expect(formatShortDate(NEW_YEAR_EVENING, { now, timeZone: CHICAGO })).toBe('January 1st 2026')
  })

  it('compares years in the viewer\'s zone', () => {
    // Still 2025 in Chicago, though already 2026 in UTC.
    const now = Date.parse('2026-01-01T02:00:00Z')
    expect(formatShortDate('2025-12-25T18:00:00Z', { now, timeZone: CHICAGO })).toBe('December 25th')
  })
})

describe('formatShortDateTime', () => {
  it('adds the time to the short date', () => {
    const now = Date.parse('2026-06-15T12:00:00Z')
    expect(formatShortDateTime(NEW_YEAR_EVENING, { now, timeZone: CHICAGO })).toBe('January 1st at 8:56pm')
  })
})

describe('formatCalendarDay', () => {
  it('names the same day whatever the viewer\'s zone', () => {
    expect(formatCalendarDay('2026-01-05')).toBe('January 5th 2026')
  })
})

describe('formatWeekday', () => {
  it('writes a day of the week by its short name', () => {
    // October 19th 2026 is a Monday.
    expect(formatWeekday(new Date(2026, 9, 19))).toBe('Mon')
  })
})

describe('formatRelativeTime', () => {
  const now = Date.parse('2026-03-10T12:00:00Z')

  it('says the largest unit a span reaches', () => {
    expect(formatRelativeTime('2026-03-10T11:55:00Z', now)).toBe('5 minutes ago')
    expect(formatRelativeTime('2026-03-10T09:00:00Z', now)).toBe('3 hours ago')
    expect(formatRelativeTime('2026-03-12T12:00:00Z', now)).toBe('in 2 days')
    expect(formatRelativeTime('2026-02-10T12:00:00Z', now)).toBe('4 weeks ago')
    expect(formatRelativeTime('2026-01-09T12:00:00Z', now)).toBe('2 months ago')
  })

  it('says "now" for this very moment', () => {
    expect(formatRelativeTime(now, now)).toBe('now')
  })
})
