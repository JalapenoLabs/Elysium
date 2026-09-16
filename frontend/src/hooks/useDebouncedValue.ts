// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useState } from 'react'

// The value as it was once it stopped changing for `delayMs`. Lets a field that asks the
// server about its contents wait until the user pauses typing.
export function useDebouncedValue<Value>(value: Value, delayMs: number) {
  const [ settled, setSettled ] = useState(value)

  useEffect(() => {
    const timer = window.setTimeout(() => setSettled(value), delayMs)
    return () => window.clearTimeout(timer)
  }, [ value, delayMs ])

  return settled
}
