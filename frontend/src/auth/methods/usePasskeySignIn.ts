// Copyright © 2026 Jalapeno Labs

// Core
import { useState, useCallback } from 'react'

// Misc
import { signInWithPasskey } from '../../api/routes/authRoutes'

// Headless passkey sign-in: wraps the native `navigator.credentials` ceremony
// (via `signInWithPasskey`) behind a busy/error state. `start` resolves `true`
// once the API has verified the assertion and set the session cookie; the caller
// re-resolves the session and routes onward.
export function usePasskeySignIn() {
  const [ isBusy, setIsBusy ] = useState(false)
  const [ error, setError ] = useState<string | null>(null)

  const start = useCallback(async (identifier: string) => {
    setIsBusy(true)
    setError(null)
    try {
      await signInWithPasskey(identifier.trim())
      return true
    }
    catch (error) {
      console.debug('passkey sign-in failed', { error })
      setError('failed')
      return false
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  return { start, isBusy, error } as const
}
