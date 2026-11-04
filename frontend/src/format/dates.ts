// Copyright © 2026 Jalapeno Labs

// Core
import i18next from 'i18next'

// Misc
import { dateTimeFormat, relativeTimeFormat } from './intl'

// Dates as people read them: "January 1st 2026", and "January 1st 2026 at 8:56pm" when the
// time matters. Months are never numbers. The server sends UTC instants; each is shown in the
// viewer's own zone, which is the only place time zones exist.
//
// `Intl` alone writes "January 1, 2026, 8:56 PM", so the parts it gives (the month's name, the
// day, the year, the hour) are arranged by the `common:dates` translations, which own the order,
// the day's ordinal suffix, and the word "at". Only typed parts are used, never `Intl`'s own
// punctuation, which changes between ICU versions.

export type DateInput = string | number | Date

type ZoneOptions = {
  // The viewer's zone unless given. Tests pin one, and a calendar day pins UTC.
  timeZone?: string
}

type ShortDateOptions = ZoneOptions & {
  // When it is now, to tell whether the date falls in this year.
  now?: number
}

// The largest unit first: a relative time is said in the largest unit it reaches.
const RELATIVE_TIME_UNITS = [
  { unit: 'year', seconds: 365 * 24 * 60 * 60 },
  { unit: 'month', seconds: 30 * 24 * 60 * 60 },
  { unit: 'week', seconds: 7 * 24 * 60 * 60 },
  { unit: 'day', seconds: 24 * 60 * 60 },
  { unit: 'hour', seconds: 60 * 60 },
  { unit: 'minute', seconds: 60 },
] as const satisfies readonly { unit: Intl.RelativeTimeFormatUnit, seconds: number }[]

function toDate(value: DateInput) {
  const date = value instanceof Date
    ? value
    : new Date(value)
  if (Number.isNaN(date.getTime())) {
    console.warn('A value to show as a date is not one, showing it as it is', { value })
    return null
  }
  return date
}

function partsOf(date: Date, options: Intl.DateTimeFormatOptions) {
  const parts: Partial<Record<Intl.DateTimeFormatPartTypes, string>> = {}
  for (const part of dateTimeFormat(options).formatToParts(date)) {
    parts[part.type] = part.value
  }
  return parts
}

// The month's name and the day with its ordinal, "January" and "1st", in the given zone.
function monthAndDay(date: Date, timeZone: string | undefined) {
  const parts = partsOf(date, { month: 'long', day: 'numeric', year: 'numeric', timeZone })
  const day = i18next.t('dates.ordinalDay', { ns: 'common', count: Number(parts.day), ordinal: true })
  return { month: parts.month, day, year: parts.year } as const
}

// "January 1st 2026".
export function formatDate(value: DateInput, options: ZoneOptions = {}) {
  const date = toDate(value)
  if (!date) {
    return String(value)
  }
  return i18next.t('dates.date', { ns: 'common', ...monthAndDay(date, options.timeZone) })
}

// "8:56pm".
export function formatTime(value: DateInput, options: ZoneOptions = {}) {
  const date = toDate(value)
  if (!date) {
    return String(value)
  }
  const parts = partsOf(date, { hour: 'numeric', minute: '2-digit', timeZone: options.timeZone })
  return i18next.t('dates.time', {
    ns: 'common',
    hour: parts.hour,
    minute: parts.minute,
    // Absent in languages that count hours to 24.
    dayPeriod: parts.dayPeriod?.toLowerCase() ?? '',
  })
}

// "January 1st 2026 at 8:56pm".
export function formatDateTime(value: DateInput, options: ZoneOptions = {}) {
  const date = toDate(value)
  if (!date) {
    return String(value)
  }
  return i18next.t('dates.dateTime', {
    ns: 'common',
    date: formatDate(date, options),
    time: formatTime(date, options),
  })
}

// "January 1st" for a date this year, for tables and lists that show the full timestamp in a
// tooltip (`DateText`). A date in another year keeps its year, since "March 3rd" alone would
// read as this March.
export function formatShortDate(value: DateInput, options: ShortDateOptions = {}) {
  const date = toDate(value)
  if (!date) {
    return String(value)
  }
  const { now = Date.now(), timeZone } = options
  const parts = monthAndDay(date, timeZone)
  if (parts.year !== partsOf(new Date(now), { year: 'numeric', timeZone }).year) {
    return i18next.t('dates.date', { ns: 'common', ...parts })
  }
  return i18next.t('dates.shortDate', { ns: 'common', month: parts.month, day: parts.day })
}

// A day with no time, such as a provider's `2026-01-05` due date. It names the same day for
// every viewer, so it is read in UTC rather than shifted into the viewer's zone, which would
// show the day before to anyone west of Greenwich.
export function formatCalendarDay(isoDay: string) {
  return formatDate(`${isoDay}T00:00:00Z`, { timeZone: 'UTC' })
}

// A day of the week's short name, "Mon", for schedules that repeat weekly.
export function formatWeekday(value: DateInput) {
  const date = toDate(value)
  if (!date) {
    return String(value)
  }
  return dateTimeFormat({ weekday: 'short' }).format(date)
}

// "5 minutes ago", "in 2 days", or "now", from `now`.
export function formatRelativeTime(value: DateInput, now: number = Date.now()) {
  const date = toDate(value)
  if (!date) {
    return String(value)
  }
  const seconds = (date.getTime() - now) / 1000
  const formatter = relativeTimeFormat({ numeric: 'auto' })
  for (const { unit, seconds: unitSeconds } of RELATIVE_TIME_UNITS) {
    if (Math.abs(seconds) >= unitSeconds) {
      return formatter.format(Math.round(seconds / unitSeconds), unit)
    }
  }
  return formatter.format(Math.round(seconds), 'second')
}
