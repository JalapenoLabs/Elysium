// Copyright © 2026 Jalapeno Labs

// Core
import { describe, expect, it } from 'vitest'

// Utility
import { CalendarDate } from '@internationalized/date'

// Misc
import { getSnoozePresetInstants, toEndOfDayInstant, toLocalDay, toSnoozeInstant } from './actionItemDates'

const NEW_YORK = 'America/New_York'

describe('toEndOfDayInstant', () => {
  it('stores a day as its last millisecond in the viewer’s zone', () => {
    const instant = toEndOfDayInstant(new CalendarDate(2026, 9, 18), NEW_YORK)

    // New York is four hours behind UTC in September.
    expect(instant).toBe('2026-09-19T03:59:59.999Z')
  })

  it('reads back as the same day', () => {
    const date = new CalendarDate(2026, 3, 8)
    const instant = toEndOfDayInstant(date, NEW_YORK)

    expect(toLocalDay(instant, NEW_YORK).toString()).toBe(date.toString())
  })
})

describe('toSnoozeInstant', () => {
  it('wakes the item at nine in the morning of that day', () => {
    expect(toSnoozeInstant(new CalendarDate(2026, 9, 21), NEW_YORK)).toBe('2026-09-21T13:00:00.000Z')
  })
})

describe('getSnoozePresetInstants', () => {
  it('offers a few hours from now, tomorrow morning, and next Monday morning', () => {
    // A Friday, 10:00 in New York.
    const now = Date.parse('2026-09-18T14:00:00.000Z')

    expect(getSnoozePresetInstants(now, NEW_YORK)).toEqual({
      laterToday: '2026-09-18T17:00:00.000Z',
      tomorrow: '2026-09-19T13:00:00.000Z',
      nextWeek: '2026-09-21T13:00:00.000Z',
    })
  })

  it('snoozes a Monday to the Monday after, never to the same day', () => {
    // A Monday, 10:00 in New York.
    const now = Date.parse('2026-09-21T14:00:00.000Z')

    expect(getSnoozePresetInstants(now, NEW_YORK).nextWeek).toBe('2026-09-28T13:00:00.000Z')
  })

  it('counts days in the viewer’s zone, not in UTC', () => {
    // Sunday 22:00 in New York is already Monday in UTC; next week is still the next day.
    const now = Date.parse('2026-09-21T02:00:00.000Z')

    expect(getSnoozePresetInstants(now, NEW_YORK).nextWeek).toBe('2026-09-21T13:00:00.000Z')
  })
})
