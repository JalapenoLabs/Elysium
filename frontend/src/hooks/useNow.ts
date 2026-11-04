// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'

// Misc
import { ACTION_ITEMS_CLOCK_TICK_MS } from '../constants'

// The current time in milliseconds, refreshed every `tickMs`, or never while it is `null`.
// Views that depend on the time (Next, overdue and snoozed markers, a running step's
// duration) read it here rather than calling Date.now() while rendering, which would be impure
// and would recompute every memoized selector each render. Only the clock is read; nothing is
// fetched.
export function useNow(tickMs: number | null = ACTION_ITEMS_CLOCK_TICK_MS) {
  const [ now, setNow ] = useState(() => Date.now())

  useEffect(() => {
    if (tickMs === null) {
      return undefined
    }
    const timer = window.setInterval(() => setNow(Date.now()), tickMs)
    return () => window.clearInterval(timer)
  }, [ tickMs ])

  return now
}
