// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'

// Misc
import { ACTION_ITEMS_CLOCK_TICK_MS } from '../constants'

// The current time in milliseconds, refreshed on a slow tick. Views that depend on the
// time (Next, overdue and snoozed markers) read it here rather than calling Date.now() while
// rendering, which would be impure and would recompute every memoized selector each render.
// Only the clock is read; nothing is fetched.
export function useNow() {
  const [ now, setNow ] = useState(() => Date.now())

  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), ACTION_ITEMS_CLOCK_TICK_MS)
    return () => window.clearInterval(timer)
  }, [])

  return now
}
