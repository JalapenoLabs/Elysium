// Copyright © 2026 Jalapeno Labs

import type { AuthMethodKind } from './types'
import type { PublicKeyCredentialCreationOptionsJSON } from '@simplewebauthn/browser'

// Core
import { useState, useRef, useCallback } from 'react'

// Lib
import { startRegistration } from '@simplewebauthn/browser'
import * as opaque from '@serenity-kit/opaque'

// How a caller wires enrollment to its endpoints. `begin` returns the server
// payload for the chosen method (a passkey challenge, or the OPAQUE registration
// response + a TOTP provisioning URI); `finish` submits the verified material and
// returns any one-time recovery codes the flow issues.
export type EnrollConfig = {
  begin: (method: AuthMethodKind, opaqueRequest?: string) => Promise<Record<string, unknown>>
  finish: (body: Record<string, unknown>) => Promise<{ recovery_codes?: string[] }>
}

// The shared passkey + OPAQUE enrollment machine, used everywhere Elysium enrolls
// a credential: first-run claim, invite acceptance, and authenticated
// add-credential. Only the endpoints differ, so callers pass an `EnrollConfig` and
// this owns the identical client-side ceremony and state. See docs/auth-model.md.
//
// The passkey path is one shot (`enrollPasskey`). The password path is two steps
// (`beginPassword` returns the TOTP provisioning URI to render as a QR, then
// `completePassword` submits the code), because the user must confirm a code
// before the credential is accepted.
export function useEnrollCredential(config: EnrollConfig) {
  // A latest-ref so the callbacks always call the current config (which closes
  // over live component state) without being recreated every render.
  const configRef = useRef(config)
  configRef.current = config

  const [ isBusy, setIsBusy ] = useState(false)
  const [ error, setError ] = useState<string | null>(null)
  const [ provisioningUri, setProvisioningUri ] = useState<string | null>(null)
  const [ recoveryCodes, setRecoveryCodes ] = useState<string[] | null>(null)

  // The OPAQUE record computed at begin, held until the code confirms at finish.
  const pendingRecord = useRef<string | null>(null)

  const enrollPasskey = useCallback(async () => {
    setError(null)
    setIsBusy(true)
    try {
      const begin = await configRef.current.begin('passkey')
      const challenge = begin.passkey as { publicKey: PublicKeyCredentialCreationOptionsJSON }
      const credential = await startRegistration({ optionsJSON: challenge.publicKey })
      const finish = await configRef.current.finish({ credential })
      setRecoveryCodes(finish.recovery_codes ?? [])
      return true
    }
    catch (error) {
      console.debug('passkey enrollment failed', { error })
      setError('failed')
      return false
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  const beginPassword = useCallback(async (password: string) => {
    setError(null)
    setIsBusy(true)
    try {
      await opaque.ready
      const registration = opaque.client.startRegistration({ password })
      const begin = await configRef.current.begin('password', registration.registrationRequest)

      const registrationResponse = begin.opaque_registration_response as string
      const { registrationRecord } = opaque.client.finishRegistration({
        clientRegistrationState: registration.clientRegistrationState,
        registrationResponse,
        password,
      })
      pendingRecord.current = registrationRecord

      const totp = begin.totp as { provisioning_uri: string }
      setProvisioningUri(totp.provisioning_uri)
      return true
    }
    catch (error) {
      console.debug('password enrollment begin failed', { error })
      setError('failed')
      return false
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  const completePassword = useCallback(async (totpCode: string) => {
    if (!pendingRecord.current) {
      console.debug('completePassword called before beginPassword')
      setError('failed')
      return false
    }
    setError(null)
    setIsBusy(true)
    try {
      const finish = await configRef.current.finish({
        opaque_registration_upload: pendingRecord.current,
        totp_code: totpCode.trim(),
      })
      setRecoveryCodes(finish.recovery_codes ?? [])
      return true
    }
    catch (error) {
      // The most likely finish failure at this step is a wrong TOTP code (400).
      console.debug('password enrollment finish failed', { error })
      setError('invalidCode')
      return false
    }
    finally {
      setIsBusy(false)
    }
  }, [])

  return {
    isBusy,
    error,
    provisioningUri,
    recoveryCodes,
    enrollPasskey,
    beginPassword,
    completePassword,
  } as const
}
