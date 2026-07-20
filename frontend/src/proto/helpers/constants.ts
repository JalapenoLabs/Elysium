// Copyright © 2026 Jalapeno Labs

// Standard display format for timestamps across Elysium: ISO-like date and time
// with the timezone abbreviation, e.g. "2026-07-07 15:30:00 MDT".
export const STANDARD_TIME_FORMAT = 'YYYY-MM-DD HH:mm:ss z'

// Compact, human-friendly instant for tables and inline text, e.g. "Jul 7, 3:30 PM".
export const STANDARD_SHORT_TIME_FORMAT = 'MMM D, h:mm A'

// The unabbreviated instant shown on hover, e.g.
// "Tuesday, July 7, 2026 at 3:30:00 PM MDT".
export const STANDARD_FULL_TIME_FORMAT = 'dddd, MMMM D, YYYY [at] h:mm:ss A z'

// Fallback timezone used when a Timestamp carries no zone. The canonical instant
// is always UTC, so UTC is the safe default for display.
export const DEFAULT_TIMEZONE = 'Etc/UTC'
