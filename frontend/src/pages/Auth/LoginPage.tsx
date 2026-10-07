// Copyright © 2026 Jalapeno Labs

import type { LoginFlow } from '@ory/client-fetch'
import type { LoginSubmission } from './loginPresentation'

// Core
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Alert, Link, Spinner } from '@heroui/react'
import { KratosMessages } from './KratosMessages'
import { PasswordSignInForm } from './PasswordSignInForm'
import { SecondFactorForm } from './SecondFactorForm'

// Misc
import { kratos } from '../../api/kratos'
import { csrfToken, readKratosFailure, toLocalPath } from '../../api/kratosFlows'
import { getLoginMode, isSecondFactorRedirect, loginCopyByMode } from './loginPresentation'
import {
  getLoginUrl,
  KRATOS_FLOW_PARAM,
  LOGIN_AAL_PARAM,
  LOGIN_NOTICE_PARAM,
  LOGIN_REFRESH_PARAM,
  LOGIN_RETURN_TO_PARAM,
  UrlTree,
} from '../../urls'

// Signing in: a password or a passkey, then the authenticator code for accounts that have
// one. The same page confirms a recent sign-in before a security change (`refresh`).
//
// Every outcome that changes who is signed in ends in a full page load, so the workspace
// starts from nothing for whoever is now signed in.
export function LoginPage() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ params ] = useSearchParams()
  const returnTo = toLocalPath(params.get(LOGIN_RETURN_TO_PARAM)) ?? UrlTree.root
  const wantsSecondFactor = params.get(LOGIN_AAL_PARAM) === 'aal2'
  const wantsRefresh = params.get(LOGIN_REFRESH_PARAM) === 'true'
  const flowId = params.get(KRATOS_FLOW_PARAM)
  const notice = params.get(LOGIN_NOTICE_PARAM)

  const [ flow, setFlow ] = useState<LoginFlow | null>(null)
  const [ failure, setFailure ] = useState<string | null>(null)
  // Bumped to start a fresh flow, such as when the old one expired.
  const [ generation, setGeneration ] = useState(0)

  useEffect(() => {
    let isCurrent = true
    const request = flowId && generation === 0
      ? kratos.getLoginFlow({ id: flowId })
      : kratos.createBrowserLoginFlow({
        aal: wantsSecondFactor
          ? 'aal2'
          : undefined,
        refresh: wantsRefresh,
      })

    request
      .then((loaded) => {
        if (isCurrent) {
          setFlow(loaded)
        }
      })
      .catch(async (error: unknown) => {
        const outcome = await readKratosFailure(error)
        if (!isCurrent) {
          return
        }
        if (outcome.kind === 'signedIn') {
          window.location.assign(returnTo)
          return
        }
        if (outcome.kind === 'expired' && generation === 0) {
          setGeneration(1)
          return
        }
        if (outcome.kind === 'redirect') {
          window.location.assign(outcome.to)
          return
        }
        setFailure(t('login.loadError'))
      })

    return () => {
      isCurrent = false
    }
  }, [ flowId, generation, wantsSecondFactor, wantsRefresh, returnTo, t ])

  // Submits one way of signing in. Resolves once the page moves on or shows why not.
  const submit = useCallback(async (body: LoginSubmission) => {
    if (!flow) {
      return
    }

    try {
      await kratos.updateLoginFlow({
        flow: flow.id,
        updateLoginFlowBody: { ...body, csrf_token: csrfToken(flow) },
      })
      // Signed in. An account with an authenticator app is asked for its code next: the
      // workspace finds the session one factor short and sends the person back here.
      window.location.assign(returnTo)
    }
    catch (error) {
      const outcome = await readKratosFailure(error)
      if (outcome.kind === 'invalid') {
        setFlow((current) => current && { ...current, ui: outcome.flow.ui })
        return
      }
      if (outcome.kind === 'signedIn') {
        window.location.assign(returnTo)
        return
      }
      if (outcome.kind === 'secondFactor' || (outcome.kind === 'redirect' && isSecondFactorRedirect(outcome.to))) {
        window.location.assign(getLoginUrl({ returnTo, secondFactor: true }))
        return
      }
      if (outcome.kind === 'redirect') {
        window.location.assign(outcome.to)
        return
      }
      if (outcome.kind === 'expired') {
        setFlow(null)
        setGeneration((current) => current + 1)
        setFailure(t('login.expired'))
        return
      }
      setFailure(outcome.kind === 'failed' && outcome.message
        ? outcome.message
        : t('common:errors.unexpected'))
    }
  }, [ flow, returnTo, t ])

  const mode = getLoginMode(flow)
  const isSecondFactor = mode === 'secondFactor'
  const isRefresh = mode === 'refresh'

  return <div>
    <h1 className='text-2xl font-bold'>{t(loginCopyByMode[mode].heading)}</h1>
    <p className='relaxed mt-1 text-sm opacity-70'>{
      t(loginCopyByMode[mode].description)
    }</p>

    {notice === 'disabled' && <Alert status='warning' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{t('login.disabledNotice')}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {failure && <Alert status='danger' className='compact'>
      <Alert.Indicator />
      <Alert.Content>
        <Alert.Description>{failure}</Alert.Description>
      </Alert.Content>
    </Alert>}

    {!flow && !failure && <div className='grid place-items-center py-10'>
      <Spinner />
    </div>}

    {flow && <>
      <KratosMessages messages={flow.ui.messages ?? []} />
      {isSecondFactor
        ? <SecondFactorForm flow={flow} onSubmit={submit} />
        : <PasswordSignInForm flow={flow} isRefresh={isRefresh} onSubmit={submit} />}
    </>}

    {!isSecondFactor && !isRefresh && <div className='mt-6 flex flex-col gap-1 text-center text-sm'>
      <Link href={UrlTree.recovery} className='mx-auto text-link'>{
        t('login.forgotPassword')
      }</Link>
      <p className='opacity-70'>
        <span>{t('login.noAccount')}</span>
        {' '}
        <Link href={UrlTree.signup} className='text-link'>{t('login.signUp')}</Link>
      </p>
    </div>}
  </div>
}
