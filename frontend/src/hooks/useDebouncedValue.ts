// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'

// `value` once it has rested for `delayMs`, so a search typed quickly is asked for once.
export function useDebouncedValue<Value>(value: Value, delayMs: number): Value {
  const [ settled, setSettled ] = useState(value)

  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(value), delayMs)
    return () => window.clearTimeout(timer)
  }, [ value, delayMs ])

  return settled
}
