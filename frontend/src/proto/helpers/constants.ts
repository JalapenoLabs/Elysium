// Copyright © 2026 Jalapeno Labs

// Standard display format for timestamps across Elysium: ISO-like date and time
// with the timezone abbreviation, e.g. "2026-07-07 15:30:00 MDT".
export const STANDARD_TIME_FORMAT = 'YYYY-MM-DD HH:mm:ss z'

// Fallback timezone used when a Timestamp carries no zone. The canonical instant
// is always UTC, so UTC is the safe default for display.
export const DEFAULT_TIMEZONE = 'Etc/UTC'
