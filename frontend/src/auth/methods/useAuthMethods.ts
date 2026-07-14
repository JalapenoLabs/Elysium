// Copyright © 2026 Jalapeno Labs

import type { AuthMethodKind } from './types'

// Core
import { useState, useCallback } from 'react'

// Misc
import { resolveAuthMethods } from '../../api/routes/authRoutes'

// Headless method discovery: resolve which sign-in methods this instance permits
// for an identifier. On failure it falls back to passkey so the gate still offers
// something, and surfaces a semantic error code the UI maps to copy.
export function useAuthMethods() {
  const [ methods, setMethods ] = useState<AuthMethodKind[] | null>(null)
  const [ isBusy, setIsBusy ] = useState(false)
  const [ error, setError ] = useState<string | null>(null)

  const resolve = useCallback(async (identifier: string) => {
    setIsBusy(true)
    setError(null)
    try {
      const response = await resolveAuthMethods(identifier.trim().toLowerCase())
      setMethods(response.methods)
      return response.methods
    }
    catch (error) {
      console.debug('method discovery failed, falling back to passkey', { error })
      setError('failed')
      const fallback: AuthMethodKind[] = [ 'passkey' ]
      setMethods(fallback)
      return fallback
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  return { methods, resolve, isBusy, error } as const
}
