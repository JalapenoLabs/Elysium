// Copyright © 2026 Jalapeno Labs

import type { CalendarDate } from '@internationalized/date'

// Utility
import { getDayOfWeek, parseAbsolute, toCalendarDate, toZoned } from '@internationalized/date'

// Misc
import { SNOOZE_LATER_TODAY_HOURS, SNOOZE_WAKE_HOUR } from '../../constants'

// Due dates and target dates are picked as days. A day is due until it ends, so it is
// stored as the last millisecond of that day in the viewer's zone: an item due today is
// overdue tomorrow, not this morning.
export function toEndOfDayInstant(date: CalendarDate, timeZone: string) {
  return toZoned(date.add({ days: 1 }), timeZone)
    .subtract({ milliseconds: 1 })
    .toAbsoluteString()
}

// The day an instant falls on in the viewer's zone, for a date picker.
export function toLocalDay(instant: string, timeZone: string) {
  return toCalendarDate(parseAbsolute(instant, timeZone))
}

// A snooze to a day wakes the item that morning rather than at midnight.
export function toSnoozeInstant(date: CalendarDate, timeZone: string) {
  return toZoned(date, timeZone)
    .set({ hour: SNOOZE_WAKE_HOUR })
    .toAbsoluteString()
}

export const SNOOZE_PRESETS = [ 'laterToday', 'tomorrow', 'nextWeek' ] as const
export type SnoozePreset = typeof SNOOZE_PRESETS[number]

// When each quick snooze ends, from `now` in the viewer's zone: a few hours from now,
// tomorrow morning, or next Monday morning.
export function getSnoozePresetInstants(now: number, timeZone: string) {
  const zonedNow = parseAbsolute(new Date(now).toISOString(), timeZone)
  const today = toCalendarDate(zonedNow)
  // Counted in a week that starts on Sunday, whatever the viewer's locale, so Sunday is 0.
  // Monday is then 1 to 7 days ahead, never today.
  const weekday = getDayOfWeek(today, 'en-US')
  const daysToMonday = ((8 - weekday) % 7) || 7

  return {
    laterToday: zonedNow.add({ hours: SNOOZE_LATER_TODAY_HOURS }).toAbsoluteString(),
    tomorrow: toSnoozeInstant(today.add({ days: 1 }), timeZone),
    nextWeek: toSnoozeInstant(today.add({ days: daysToMonday }), timeZone),
  } as const satisfies Record<SnoozePreset, string>
}
