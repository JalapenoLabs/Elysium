// Copyright © 2026 Jalapeno Labs

import type { DailyHours } from './hours'

// Core
import { describe, expect, it } from 'vitest'

// Misc
import { MINUTES_PER_WEEK, fromUtcWindows, toUtcWindows } from './hours'

const NINE = 9 * 60
const FIVE_PM = 17 * 60
const WEEKDAYS_NINE_TO_FIVE: DailyHours = { days: [ 0, 1, 2, 3, 4 ], startMinute: NINE, endMinute: FIVE_PM }

describe('toUtcWindows', () => {
  it('keeps the hours as they are in UTC', () => {
    expect(toUtcWindows({ days: [ 0 ], startMinute: NINE, endMinute: FIVE_PM }, 0)).toEqual([
      { startMinute: NINE, endMinute: FIVE_PM },
    ])
  })

  it('moves hours behind UTC later in the UTC week', () => {
    // 09:00 to 17:00 on Monday in UTC-5 is 14:00 to 22:00 UTC.
    expect(toUtcWindows({ days: [ 0 ], startMinute: NINE, endMinute: FIVE_PM }, -300)).toEqual([
      { startMinute: 14 * 60, endMinute: 22 * 60 },
    ])
  })

  it('splits a range that crosses the end of the UTC week', () => {
    // Sunday 22:00 to 23:30 in UTC-2 is Monday 00:00 to 01:30 UTC, at the start of the week;
    // Sunday 20:00 to 23:00 in UTC-3 runs from Sunday 23:00 to Monday 02:00 UTC.
    expect(toUtcWindows({ days: [ 6 ], startMinute: 20 * 60, endMinute: 23 * 60 }, -180)).toEqual([
      { startMinute: MINUTES_PER_WEEK - 60, endMinute: MINUTES_PER_WEEK },
      { startMinute: 0, endMinute: 120 },
    ])
  })
})

describe('fromUtcWindows', () => {
  it('reads back the daily range it was saved from, in any zone', () => {
    for (const offset of [ -480, -300, 0, 60, 330, 600 ]) {
      const windows = toUtcWindows(WEEKDAYS_NINE_TO_FIVE, offset)
      expect(fromUtcWindows(windows, offset)).toEqual(WEEKDAYS_NINE_TO_FIVE)
    }
  })

  it('rejoins a range the end of the week split', () => {
    const sunday: DailyHours = { days: [ 6 ], startMinute: 20 * 60, endMinute: 23 * 60 }
    expect(fromUtcWindows(toUtcWindows(sunday, -180), -180)).toEqual(sunday)
  })

  it('reads nothing as no schedule', () => {
    expect(fromUtcWindows([], 0)).toBeNull()
  })

  it('refuses windows that are not one range on whole days', () => {
    expect(fromUtcWindows([
      { startMinute: NINE, endMinute: FIVE_PM },
      { startMinute: 1440 + NINE, endMinute: 1440 + 12 * 60 },
    ], 0)).toBeNull()
  })
})
