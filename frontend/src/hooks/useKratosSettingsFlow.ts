// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow, UpdateSettingsFlowBody } from '@ory/client-fetch'

// Core
import { useCallback, useEffect, useEffectEvent, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import { mutate } from 'swr'
import { useKratosMessageToasts } from './useKratosMessageToasts'

// Misc
import { kratos } from '../api/kratos'
import { csrfToken, readKratosFailure } from '../api/kratosFlows'
import { LEAVE_WITHOUT_ASKING } from '../layout/page/pageSave'
import { toast } from '../toast/toast'
import { getLoginUrl, KRATOS_FLOW_PARAM } from '../urls'

// The changes My account and Security make, as Kratos takes each submission.
export type SettingsSubmission = Extract<
  UpdateSettingsFlowBody,
  { method: 'profile' | 'password' | 'totp' | 'lookup_secret' | 'passkey' }
>

type Options = {
  // The address's parameter the flow's id lives in. Given one, the hook writes the id of a flow
  // it starts there, so reloading the page picks the same flow up again with whatever it holds,
  // such as lookup codes made but not yet saved. A page running two flows names one for each.
  // Without it, the hook reads Kratos's own `flow` parameter, which a recovery link carries, and
  // writes nothing.
  flowParam?: string
}

// A Kratos settings flow: every change to the profile, the password, passkeys, the
// authenticator app, and lookup codes is one submission of it. `returnTo` is the page using
// it, where a change that needs a recent sign-in, or the second factor, comes back to.
//
// A recovery link lands on Security with its flow in the address, already signed
// in. Kratos's messages for the whole form become toasts (`useKratosMessageToasts`), so
// nothing they say moves the page; a field's own message stays on its field.
export function useKratosSettingsFlow(returnTo: string, options: Options = {}) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ params, setParams ] = useSearchParams()
  const flowParam = options.flowParam ?? KRATOS_FLOW_PARAM
  const writesFlowId = Boolean(options.flowParam)
  const flowId = params.get(flowParam)
  const [ flow, setFlow ] = useState<SettingsFlow | null>(null)
  const [ loadFailed, setLoadFailed ] = useState(false)
  // Bumped to start a fresh flow when the current one expired.
  const [ generation, setGeneration ] = useState(0)
  // The id of the flow held now, so writing it into the address does not load it over again.
  const heldFlowId = useRef<string | null>(null)

  const toastFormMessages = useKratosMessageToasts()

  // Outside the effect's dependencies: the address changing for any other reason, such as a
  // tab, must not load the flow again. Recording the flow stays on the page, so a page with
  // unsaved work does not ask about leaving it.
  const writeFlowId = useEffectEvent((id: string) => {
    if (!writesFlowId) {
      return
    }
    setParams((current) => {
      const next = new URLSearchParams(current)
      next.set(flowParam, id)
      return next
    }, { replace: true, state: LEAVE_WITHOUT_ASKING })
  })

  useEffect(() => {
    if (flowId && flowId === heldFlowId.current) {
      return undefined
    }
    let isCurrent = true
    const request = flowId && generation === 0
      ? kratos.getSettingsFlow({ id: flowId })
      : kratos.createBrowserSettingsFlow()
    request
      .then((loaded) => {
        if (!isCurrent) {
          return
        }
        heldFlowId.current = loaded.id
        setFlow(loaded)
        setLoadFailed(false)
        // Such as the notice a recovery link opens with.
        toastFormMessages(loaded.ui.messages)
        writeFlowId(loaded.id)
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

  // Starts a fresh flow in place of the one held.
  const restart = useCallback(() => {
    heldFlowId.current = null
    setGeneration((current) => current + 1)
  }, [])

  // Submits one change and says whether it was saved, confirming it with `savedMessage`, or with
  // nothing when it is null, such as a step of a longer change. A refusal Kratos explains shows
  // on its field, or as a toast for the whole form; anything else is a toast.
  const submit = useCallback(async (body: SettingsSubmission, savedMessage: string | null) => {
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
      if (savedMessage) {
        toast.success(savedMessage)
      }
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
        restart()
        toast.warning(t('security.expired'))
        return false
      }
      console.debug('useKratosSettingsFlow could not save a change', { outcome })
      toast.danger(t('common:errors.unexpected'))
      return false
    }
  }, [ flow, restart, returnTo, t, toastFormMessages ])

  return {
    flow,
    loadFailed,
    submit,
    retry: restart,
  } as const
}

export type KratosSettingsFlow = ReturnType<typeof useKratosSettingsFlow>
