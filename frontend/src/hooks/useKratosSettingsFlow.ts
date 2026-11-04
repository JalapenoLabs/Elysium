// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow, UpdateSettingsFlowBody } from '@ory/client-fetch'

// Core
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { mutate } from 'swr'
import { useKratosMessageToasts } from './useKratosMessageToasts'

// Misc
import { kratos } from '../api/kratos'
import { csrfToken, readKratosFailure } from '../api/kratosFlows'
import { toast } from '../toast/toast'
import { getLoginUrl, KRATOS_FLOW_PARAM } from '../urls'

// The changes My account and Security make, as Kratos takes each submission.
export type SettingsSubmission = Extract<
  UpdateSettingsFlowBody,
  { method: 'profile' | 'password' | 'totp' | 'lookup_secret' | 'passkey' }
>

// A Kratos settings flow: every change to the profile, the password, passkeys, the
// authenticator app, and lookup codes is one submission of it. `returnTo` is the page using
// it, where a change that needs a recent sign-in, or the second factor, comes back to.
//
// A recovery link lands on Security with its flow in the address, already signed
// in. Kratos's messages for the whole form become toasts (`useKratosMessageToasts`), so
// nothing they say moves the page; a field's own message stays on its field.
export function useKratosSettingsFlow(returnTo: string) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ params ] = useSearchParams()
  const flowId = params.get(KRATOS_FLOW_PARAM)
  const [ flow, setFlow ] = useState<SettingsFlow | null>(null)
  const [ loadFailed, setLoadFailed ] = useState(false)
  // Bumped to start a fresh flow when the current one expired.
  const [ generation, setGeneration ] = useState(0)

  const toastFormMessages = useKratosMessageToasts()

  useEffect(() => {
    let isCurrent = true
    const request = flowId && generation === 0
      ? kratos.getSettingsFlow({ id: flowId })
      : kratos.createBrowserSettingsFlow()
    request
      .then((loaded) => {
        if (!isCurrent) {
          return
        }
        setFlow(loaded)
        setLoadFailed(false)
        // Such as the notice a recovery link opens with.
        toastFormMessages(loaded.ui.messages)
      })
      .catch(async (error: unknown) => {
        const outcome = await readKratosFailure(error)
        if (!isCurrent) {
          return
        }
        if (outcome.kind === 'secondFactor') {
          window.location.assign(getLoginUrl({ returnTo, secondFactor: true }))
          return
        }
        if (outcome.kind === 'signedOut') {
          window.location.assign(getLoginUrl({ returnTo }))
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
  }, [ flowId, generation, returnTo, toastFormMessages ])

  // Submits one change and says whether it was saved. A refusal Kratos explains shows on its
  // field, or as a toast for the whole form; anything else is a toast.
  const submit = useCallback(async (body: SettingsSubmission, savedMessage: string) => {
    if (!flow) {
      console.debug('useKratosSettingsFlow was asked to submit before its flow loaded')
      return false
    }

    try {
      const updated = await kratos.updateSettingsFlow({
        flow: flow.id,
        updateSettingsFlowBody: { ...body, csrf_token: csrfToken(flow) },
      })
      setFlow(updated)
      toast.success(savedMessage)
      // A name or an authenticator changed changes what the workspace shows and asks of
      // this person.
      void mutate('v1/me')
      return true
    }
    catch (error) {
      const outcome = await readKratosFailure(error)
      if (outcome.kind === 'invalid') {
        setFlow({ ...flow, ui: outcome.flow.ui })
        toastFormMessages(outcome.flow.ui.messages)
        return false
      }
      if (outcome.kind === 'signedOut') {
        window.location.assign(getLoginUrl({ returnTo }))
        return false
      }
      if (outcome.kind === 'refresh') {
        window.location.assign(getLoginUrl({ returnTo, refresh: true }))
        return false
      }
      if (outcome.kind === 'secondFactor') {
        window.location.assign(getLoginUrl({ returnTo, secondFactor: true }))
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
      console.debug('useKratosSettingsFlow could not save a change', { outcome })
      toast.danger(t('common:errors.unexpected'))
      return false
    }
  }, [ flow, returnTo, t, toastFormMessages ])

  return {
    flow,
    loadFailed,
    submit,
    retry: () => setGeneration((current) => current + 1),
  } as const
}

export type KratosSettingsFlow = ReturnType<typeof useKratosSettingsFlow>
