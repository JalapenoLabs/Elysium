// Copyright © 2026 Jalapeno Labs

// Core
import { useState, useCallback } from 'react'

// Lib
import * as opaque from '@serenity-kit/opaque'

// Misc
import {
  passwordAuthenticateBegin,
  passwordAuthenticateFinish,
  verifyTotp,
} from '../../api/routes/authRoutes'

// Where the password sign-in machine currently is. `'password'` awaits the
// password, `'totp'` awaits the second-factor code, `'done'` means the session
// cookie is set and the caller should re-resolve the session.
type Stage = 'password' | 'totp' | 'done'

// Reads a ky HTTPError's status without asserting types across the boundary.
function statusOf(error: unknown): number | undefined {
  const response = (error as { response?: Response })?.response
  return response?.status
}

// Headless OPAQUE password sign-in with the TOTP second factor. The password
// never leaves the browser: `@serenity-kit/opaque` runs the KE1/KE2/KE3 exchange
// and the server only ever sees opaque messages. Errors surface as semantic codes
// (`'invalid'`, `'rateLimited'`, `'invalidCode'`, `'failed'`) the UI maps to copy.
export function usePasswordSignIn() {
  const [ stage, setStage ] = useState<Stage>('password')
  const [ error, setError ] = useState<string | null>(null)
  const [ isBusy, setIsBusy ] = useState(false)

  const submitPassword = useCallback(async (identifier: string, password: string) => {
    setError(null)
    setIsBusy(true)
    try {
      await opaque.ready
      const { clientLoginState, startLoginRequest } = opaque.client.startLogin({ password })
      const { ke2 } = await passwordAuthenticateBegin(
        identifier.trim().toLowerCase(),
        startLoginRequest,
      )

      // A wrong password yields no valid finalization: the client can tell before
      // ever sending KE3, so an invalid password never reaches the server twice.
      const result = opaque.client.finishLogin({ clientLoginState, loginResponse: ke2, password })
      if (!result) {
        setError('invalid')
        return
      }

      const response = await passwordAuthenticateFinish(result.finishLoginRequest)
      if (response.status === 204) {
        setStage('done')
        return
      }
      if (response.status === 401) {
        const body = await response.json().catch(() => ({})) as { reason?: string }
        if (body.reason === 'totp_required') {
          setStage('totp')
          return
        }
        setError('invalid')
        return
      }
      setError('failed')
    }
    catch (error) {
      if (statusOf(error) === 429) {
        setError('rateLimited')
      }
      else {
        console.debug('password sign-in failed', { error })
        setError('failed')
      }
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  const submitTotp = useCallback(async (code: string) => {
    setError(null)
    setIsBusy(true)
    try {
      const response = await verifyTotp(code.trim())
      if (response.status === 204) {
        setStage('done')
        return
      }
      setError('invalidCode')
    }
    catch (error) {
      console.debug('totp verification failed', { error })
      setError('failed')
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  // Return the machine to its initial state. Call this when the user backs out to a
  // different identifier so a stale `'totp'` stage does not carry over. The server's
  // pending cookie is short-lived and a fresh `submitPassword` supersedes it, so
  // this is purely client-side.
  const reset = useCallback(() => {
    setStage('password')
    setError(null)
    setIsBusy(false)
  }, [])

  return { stage, error, isBusy, submitPassword, submitTotp, reset } as const
}
