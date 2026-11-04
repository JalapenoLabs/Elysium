// Copyright © 2026 Jalapeno Labs

// Misc
import { listFormat, numberFormat } from './intl'

// How long something took, from a count of milliseconds. Each unit is written by `Intl`'s
// narrow unit style and joined by its unit list, so the result reads "2h 5m" in English and
// follows each language's own abbreviations elsewhere.

const MILLISECONDS_PER_SECOND = 1000
const MILLISECONDS_PER_MINUTE = 60 * MILLISECONDS_PER_SECOND
const MILLISECONDS_PER_HOUR = 60 * MILLISECONDS_PER_MINUTE

function unitAmount(value: number, unit: 'hour' | 'minute' | 'second' | 'millisecond', fractionDigits = 0) {
  return numberFormat({ style: 'unit', unit, unitDisplay: 'narrow', maximumFractionDigits: fractionDigits })
    .format(value)
}

// "850ms", "2.4s", "3m 4s", "2h 5m": the two largest units from a minute up, and seconds to one
// decimal below it. A span measured against the viewer's clock can come out a little negative
// when that clock runs behind the server's, so it counts from zero.
export function formatDuration(milliseconds: number) {
  const span = Math.max(0, milliseconds)
  if (span < MILLISECONDS_PER_SECOND) {
    return unitAmount(Math.round(span), 'millisecond')
  }
  if (span < MILLISECONDS_PER_MINUTE) {
    return unitAmount(span / MILLISECONDS_PER_SECOND, 'second', 1)
  }

  const hours = Math.floor(span / MILLISECONDS_PER_HOUR)
  const minutes = Math.floor((span % MILLISECONDS_PER_HOUR) / MILLISECONDS_PER_MINUTE)
  const seconds = Math.floor((span % MILLISECONDS_PER_MINUTE) / MILLISECONDS_PER_SECOND)
  const units = hours
    ? [ unitAmount(hours, 'hour'), unitAmount(minutes, 'minute') ]
    : [ unitAmount(minutes, 'minute'), unitAmount(seconds, 'second') ]
  return listFormat({ type: 'unit', style: 'narrow' }).format(units)
}

// A point in a recording as a player shows it: "3:04", or "1:02:03" from an hour.
export function formatMediaPosition(milliseconds: number) {
  const totalSeconds = Math.floor(Math.max(0, milliseconds) / MILLISECONDS_PER_SECOND)
  const hours = Math.floor(totalSeconds / 3600)
  const minutes = Math.floor((totalSeconds % 3600) / 60)
  const seconds = String(totalSeconds % 60).padStart(2, '0')
  if (hours) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${seconds}`
  }
  return `${minutes}:${seconds}`
}
