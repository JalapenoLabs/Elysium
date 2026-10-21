// Copyright © 2026 Jalapeno Labs

import type { AvailabilityWindow } from '../api/routes/elysiaRoutes'

// Elysia's hours as a person edits them, and as the server stores them.
//
// The server holds no time zones, so her hours are windows of UTC minutes in the week, where
// Monday 00:00 UTC is minute 0 (api/src/elysia/availability.rs). A person edits one daily range
// on chosen days in their own zone; these functions convert between the two with the zone's
// current offset. A schedule saved before daylight saving changes therefore moves by an hour
// in local time until it is saved again, which the editor says.

export const MINUTES_PER_DAY = 24 * 60
export const MINUTES_PER_WEEK = 7 * MINUTES_PER_DAY

// Local days, Monday first, as the server counts the week.
export const WEEKDAYS = [ 0, 1, 2, 3, 4, 5, 6 ] as const
export type Weekday = typeof WEEKDAYS[number]

// One daily range on chosen local days. `startMinute` and `endMinute` are minutes into the
// local day, start before end.
export type DailyHours = {
  days: Weekday[]
  startMinute: number
  endMinute: number
}

// How far the viewer's zone is ahead of UTC right now, in minutes (UTC-5 is -300).
export function localOffsetMinutes(now = new Date()) {
  return -now.getTimezoneOffset()
}

function wrapWeek(minute: number) {
  return ((minute % MINUTES_PER_WEEK) + MINUTES_PER_WEEK) % MINUTES_PER_WEEK
}

// The UTC windows for `hours` in a zone `offsetMinutes` ahead of UTC. A range that crosses the
// end of the UTC week becomes two windows, as the server requires.
export function toUtcWindows(hours: DailyHours, offsetMinutes: number): AvailabilityWindow[] {
  const windows: AvailabilityWindow[] = []
  for (const day of [ ...hours.days ].sort((first, second) => first - second)) {
    const start = wrapWeek(day * MINUTES_PER_DAY + hours.startMinute - offsetMinutes)
    const length = hours.endMinute - hours.startMinute
    const end = start + length
    if (end <= MINUTES_PER_WEEK) {
      windows.push({ startMinute: start, endMinute: end })
      continue
    }
    windows.push({ startMinute: start, endMinute: MINUTES_PER_WEEK })
    windows.push({ startMinute: 0, endMinute: end - MINUTES_PER_WEEK })
  }
  return windows
}

// The daily range `windows` describe in a zone `offsetMinutes` ahead of UTC, or null when they
// are not one range on whole days, such as a schedule saved through the API by hand.
export function fromUtcWindows(windows: AvailabilityWindow[], offsetMinutes: number): DailyHours | null {
  if (!windows.length) {
    return null
  }

  // Rejoin a range the end of the week split in two: the last window running to the end of
  // the week continues as the first one starting at minute 0.
  const joined = [ ...windows ].sort((first, second) => first.startMinute - second.startMinute)
  const first = joined[0]
  const last = joined.at(-1)
  if (joined.length > 1 && first?.startMinute === 0 && last?.endMinute === MINUTES_PER_WEEK) {
    joined.shift()
    joined[joined.length - 1] = {
      startMinute: last.startMinute,
      endMinute: MINUTES_PER_WEEK + first.endMinute,
    }
  }

  const days: Weekday[] = []
  let startMinute: number | null = null
  let endMinute: number | null = null
  for (const window of joined) {
    const localStart = wrapWeek(window.startMinute + offsetMinutes)
    const length = window.endMinute - window.startMinute
    const day = Math.floor(localStart / MINUTES_PER_DAY)
    const dayStart = localStart % MINUTES_PER_DAY
    const dayEnd = dayStart + length
    if (dayEnd > MINUTES_PER_DAY) {
      return null
    }
    if (startMinute === null) {
      startMinute = dayStart
      endMinute = dayEnd
    }
    if (dayStart !== startMinute || dayEnd !== endMinute) {
      return null
    }
    const weekday = WEEKDAYS[day]
    if (weekday === undefined) {
      return null
    }
    days.push(weekday)
  }
  if (startMinute === null || endMinute === null) {
    return null
  }
  return { days: days.sort((first, second) => first - second), startMinute, endMinute }
}
