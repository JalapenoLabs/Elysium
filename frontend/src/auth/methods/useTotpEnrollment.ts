// Copyright © 2026 Jalapeno Labs

// Core
import { useCallback, useEffect, useState } from 'react'

// Misc
import { beginTotpEnrollment, confirmTotpEnrollment } from '../../api/routes/userRoutes'

// The authenticator-app enrollment machine for the signed-in account, shared by
// the require-2FA gate and the Security Center. `begin` fetches the provisioning
// URI to render as a QR; `confirm` stores the secret once the current code
// verifies. Errors surface as semantic codes the UI maps to copy.
export function useTotpEnrollment() {
  const [ provisioningUri, setProvisioningUri ] = useState<string | null>(null)
  const [ isBusy, setIsBusy ] = useState(false)
  const [ error, setError ] = useState<string | null>(null)

  // Kicks off enrollment; called on mount by the gate, or on open by the modal.
  const begin = useCallback(async () => {
    setError(null)
    setIsBusy(true)
    try {
      const response = await beginTotpEnrollment()
      setProvisioningUri(response.provisioning_uri)
      return true
    }
    catch (error) {
      console.debug('totp enrollment begin failed', { error })
      setError('failed')
      return false
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  // Confirms the code. Resolves true once stored; the caller re-resolves the
  // session (which clears `totp_enrollment_required`).
  const confirm = useCallback(async (code: string) => {
    setError(null)
    setIsBusy(true)
    try {
      const response = await confirmTotpEnrollment(code)
      if (response.status === 204) {
        return true
      }
      setError('invalidCode')
      return false
    }
    catch (error) {
      // A wrong or expired code is the expected failure here.
      console.debug('totp enrollment confirm failed', { error })
      setError('invalidCode')
      return false
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  return { provisioningUri, isBusy, error, begin, confirm } as const
}

// A convenience wrapper that begins enrollment as soon as it mounts, for
// surfaces (the gate) that always want a QR immediately.
export function useAutoStartedTotpEnrollment() {
  const enrollment = useTotpEnrollment()
  const { begin } = enrollment

  useEffect(() => {
    void begin()
  }, [ begin ])

  return enrollment
}
