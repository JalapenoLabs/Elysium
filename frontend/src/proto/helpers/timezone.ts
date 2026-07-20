// Copyright © 2026 Jalapeno Labs

// Core
import moment from 'moment-timezone'

// Misc
import { DEFAULT_TIMEZONE } from './constants'

// The timezone a timestamp should be *displayed* in for the current viewer.
//
// Instants are canonical UTC everywhere they are stored and transported (the
// database and the wire); which zone to render them in is a per-user display
// choice, never baked into the data. A non-empty saved preference wins; an empty
// one (the default) auto-detects from the browser. UTC is returned only when it
// is genuinely the detected or chosen zone, never as a blanket fallback.
export function resolveDisplayTimezone(userTimezone?: string): string {
  const preference = userTimezone?.trim()
  if (preference) {
    return preference
  }
  return moment.tz.guess() || DEFAULT_TIMEZONE
}

// Every IANA zone the client knows, each labelled with its current UTC offset so
// a ~600-entry picker stays scannable ("America/Denver (GMT-06:00)"). The zone
// itself is the saved value; the label is only for reading.
export function buildTimezoneOptions() {
  return moment.tz.names().map((zone) => ({
    zone,
    label: `${zone} (GMT${moment.tz(zone).format('Z')})`,
  }))
}
