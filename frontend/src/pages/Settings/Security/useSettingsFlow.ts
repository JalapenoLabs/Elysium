// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow, UpdateSettingsFlowBody } from '@ory/client-fetch'

// Core
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { mutate } from 'swr'

// User interface
import { toast } from '@heroui/react'

// Misc
import { kratos } from '../../../api/kratos'
import { csrfToken, readKratosFailure } from '../../../api/kratosFlows'
import { getLoginUrl, KRATOS_FLOW_PARAM, UrlTree } from '../../../urls'

// The changes the Sign-in & security page makes, as Kratos takes each submission.
export type SettingsSubmission = Extract<
  UpdateSettingsFlowBody,
  { method: 'profile' | 'password' | 'totp' | 'lookup_secret' | 'passkey' }
>

// The Kratos settings flow behind Sign-in & security: every change to the profile, the
// password, passkeys, the authenticator app, and lookup codes is one submission of it.
//
// A recovery link lands here with its flow in the address, already signed in. A change that
// needs a recent sign-in sends the person to confirm it, and back.
export function useSettingsFlow() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ params ] = useSearchParams()
  const flowId = params.get(KRATOS_FLOW_PARAM)
  const [ flow, setFlow ] = useState<SettingsFlow | null>(null)
  const [ loadFailed, setLoadFailed ] = useState(false)
  // Bumped to start a fresh flow when the current one expired.
  const [ generation, setGeneration ] = useState(0)

  useEffect(() => {
    let isCurrent = true
    const request = flowId && generation === 0
      ? kratos.getSettingsFlow({ id: flowId })
      : kratos.createBrowserSettingsFlow()
    request
      .then((loaded) => {
        if (isCurrent) {
          setFlow(loaded)
          setLoadFailed(false)
        }
      })
      .catch(async (error: unknown) => {
        const outcome = await readKratosFailure(error)
        if (!isCurrent) {
          return
        }
        if (outcome.kind === 'secondFactor') {
          window.location.assign(getLoginUrl({ returnTo: UrlTree.settingsSecurity, secondFactor: true }))
          return
        }
        if (outcome.kind === 'signedOut') {
          window.location.assign(getLoginUrl({ returnTo: UrlTree.settingsSecurity }))
          return
        }
        if (outcome.kind === 'redirect') {
          window.location.assign(outcome.to)
          return
        }
        if (outcome.kind === 'expired' && generation === 0) {
          setGeneration(1)
          return
        }
        setLoadFailed(true)
      })
    return () => {
      isCurrent = false
    }
  }, [ flowId, generation ])

  // Submits one change and says whether it was saved. A refusal Kratos explains shows on the
  // form; anything else is a toast.
  const submit = useCallback(async (body: SettingsSubmission, savedMessage: string) => {
    if (!flow) {
      return false
    }

    try {
      const updated = await kratos.updateSettingsFlow({
        flow: flow.id,
        updateSettingsFlowBody: { ...body, csrf_token: csrfToken(flow) },
      })
      setFlow(updated)
      toast.success(savedMessage)
      // An authenticator set up or removed changes what the workspace asks of this person.
      void mutate('v1/me')
      return true
    }
    catch (error) {
      const outcome = await readKratosFailure(error)
      if (outcome.kind === 'invalid') {
        setFlow({ ...flow, ui: outcome.flow.ui })
        return false
      }
      if (outcome.kind === 'signedOut') {
        window.location.assign(getLoginUrl({ returnTo: UrlTree.settingsSecurity }))
        return false
      }
      if (outcome.kind === 'refresh') {
        window.location.assign(getLoginUrl({ returnTo: UrlTree.settingsSecurity, refresh: true }))
        return false
      }
      if (outcome.kind === 'secondFactor') {
        window.location.assign(getLoginUrl({ returnTo: UrlTree.settingsSecurity, secondFactor: true }))
        return false
      }
      if (outcome.kind === 'redirect') {
        window.location.assign(outcome.to)
        return false
      }
      if (outcome.kind === 'expired') {
        setFlow(null)
        setGeneration((current) => current + 1)
        toast.warning(t('security.expired'))
        return false
      }
      console.debug('useSettingsFlow could not save a change', { outcome })
      toast.danger(t('common:errors.unexpected'))
      return false
    }
  }, [ flow, t ])

  return {
    flow,
    loadFailed,
    submit,
    retry: () => setGeneration((current) => current + 1),
  } as const
}

export type SettingsFlowControls = ReturnType<typeof useSettingsFlow>
