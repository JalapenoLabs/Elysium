// Copyright © 2026 Jalapeno Labs

// Core
import { useEffect, useEffectEvent, useState } from 'react'

// Misc
import { RECORD_SEARCH_DELAY_MS } from '../constants'

// What the API last answered, and for which request.
type Settled<Answer> = {
  key: string
  answer: Answer | null
  hasFailed: boolean
}

export type DebouncedAnswer<Answer> = {
  // The answer to the current request, or null while it is loading, failed, or not asked.
  answer: Answer | null
  isLoading: boolean
  hasFailed: boolean
}

const NOTHING_ASKED = { answer: null, isLoading: false, hasFailed: false } as const

// Asks the API for `key`'s answer 300ms after `key` stops changing (ux.md, Search inputs), and
// cancels a request a newer key replaces, so a slow answer to an older search can never land
// over a newer one. `key` names everything the request depends on, and null asks nothing. From
// the moment `key` changes until its own answer arrives it reports `isLoading`, rather than
// showing an older key's answer. `request` is read when the delay runs out, with the signal
// that cancels it.
export function useDebouncedRequest<Answer>(
  key: string | null,
  request: (signal: AbortSignal) => Promise<Answer>,
): DebouncedAnswer<Answer> {
  const [ settled, setSettled ] = useState<Settled<Answer> | null>(null)
  const send = useEffectEvent((signal: AbortSignal) => request(signal))

  useEffect(() => {
    // Nothing to ask for; the answer below says so without a request.
    if (key === null) {
      return undefined
    }

    const controller = new AbortController()
    const timer = window.setTimeout(async () => {
      try {
        const answer = await send(controller.signal)
        // An answer that arrived as a newer key replaced it must not land over that one's.
        if (controller.signal.aborted) {
          return
        }
        setSettled({ key, answer, hasFailed: false })
      }
      catch (error) {
        // A newer key replaced this one; its own request is already on the way.
        if (controller.signal.aborted) {
          return
        }
        console.debug('useDebouncedRequest could not answer a request', { key, error })
        setSettled({ key, answer: null, hasFailed: true })
      }
    }, RECORD_SEARCH_DELAY_MS)

    return () => {
      window.clearTimeout(timer)
      controller.abort()
    }
  }, [ key ])

  if (key === null) {
    return NOTHING_ASKED
  }
  if (settled?.key !== key) {
    return { answer: null, isLoading: true, hasFailed: false }
  }
  return { answer: settled.answer, isLoading: false, hasFailed: settled.hasFailed }
}
