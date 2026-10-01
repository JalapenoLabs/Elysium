// Copyright © 2026 Jalapeno Labs

import type { ChallengeFailure } from './oauthPresentation'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'
import useSWR from 'swr'

// Redux
import { selectMe } from '../../store/authSlice'
import { useAppSelector } from '../../store/hooks'

// User interface
import { Button, Spinner } from '@heroui/react'
import { LuCheck, LuPlug } from 'react-icons/lu'
import { ChallengeFailureNotice } from './ChallengeFailureNotice'

// Misc
import { acceptOAuthConsent, getOAuthConsent, rejectOAuthConsent } from '../../api/routes/oauthRoutes'
import { challengeFailure, clientHost, knownScopes, scopeLabelKeys } from './oauthPresentation'

// Where Hydra sends someone connecting an MCP client to approve it: what the client is, what it
// may do as them, and Allow or Deny. Either way the browser goes back to the client.
export function OAuthConsentPage() {
  const { t } = useTranslation('oauth')
  const me = useAppSelector(selectMe)
  const [ params ] = useSearchParams()
  const challenge = params.get('consent_challenge')
  const [ answerFailure, setAnswerFailure ] = useState<ChallengeFailure | null>(null)
  const [ answering, setAnswering ] = useState<'allow' | 'deny' | null>(null)

  const { data, error } = useSWR(
    challenge
      ? [ 'v1/oauth/consent', challenge ]
      : null,
    ([ , key ]) => getOAuthConsent(key),
  )

  // A client already connected for these scopes is let through without asking, through the same
  // POST as pressing Allow, so no change happens on a page load.
  const alreadyApproved = Boolean(data?.alreadyApproved && data.scopes.length)
  useEffect(() => {
    if (alreadyApproved) {
      void answer('allow')
    }
    // `answer` reads only the challenge, which this page never changes.
  }, [ alreadyApproved ])

  async function answer(choice: 'allow' | 'deny') {
    if (!challenge) {
      return
    }
    setAnswering(choice)
    try {
      const response = choice === 'allow'
        ? await acceptOAuthConsent(challenge)
        : await rejectOAuthConsent(challenge)
      window.location.assign(response.redirectTo)
    }
    catch (failure) {
      console.debug('OAuthConsentPage could not answer the consent', { failure, choice })
      setAnswerFailure(challengeFailure(failure))
      setAnswering(null)
    }
  }

  if (!challenge) {
    return <ChallengeFailureNotice failure='expired' />
  }
  if (answerFailure || error) {
    return <ChallengeFailureNotice failure={answerFailure ?? challengeFailure(error)} />
  }
  if (!data || alreadyApproved) {
    return <div className='grid place-items-center gap-4 py-8 text-center'>
      <Spinner />
      {alreadyApproved && <p className='text-sm opacity-70'>{t('consent.redirecting')}</p>}
    </div>
  }

  const clientName = data.client.name || t('consent.unnamedClient')
  const host = clientHost(data.client)
  const sendsTo = data.client.redirectHosts.join(', ')

  // Asking for nothing a person can grant: nothing to approve, only to send the app back.
  if (!data.scopes.length) {
    return <div className='text-center'>
      <LuPlug className='relaxed mx-auto size-9 opacity-60' aria-hidden />
      <h1 className='compact text-xl font-bold'>{
        t('consent.nothingGrantable.heading', { client: clientName })
      }</h1>
      <p className='relaxed text-sm opacity-70'>{t('consent.nothingGrantable.message')}</p>
      <Button
        className='w-full'
        variant='outline'
        isPending={answering === 'deny'}
        isDisabled={answering !== null}
        onPress={() => answer('deny')}
      >
        <span>{t('consent.nothingGrantable.return')}</span>
      </Button>
    </div>
  }

  return <div>
    <div className='relaxed text-center'>
      <LuPlug className='relaxed mx-auto size-9 text-accent' aria-hidden />
      <h1 className='text-xl font-bold'>{
        t('consent.heading', { client: clientName })
      }</h1>
      {host && <p className='mt-1 text-xs opacity-60'>{t('consent.from', { host })}</p>}
    </div>

    {/* The name and website are whatever the app registered with. Where the code goes is not. */}
    <div className='relaxed rounded-lg border border-separator px-3 py-2 text-xs'>
      <p className='font-semibold'>{t('consent.sendsTo', { hosts: sendsTo })}</p>
      <p className='mt-1 opacity-70'>{t('consent.checkSendsTo')}</p>
      <p className='mt-1 break-all opacity-50'>{t('consent.clientId', { id: data.client.id })}</p>
    </div>

    <p className='compact text-sm'>{
      t('consent.actsAs', { name: me?.user.name ?? '' })
    }</p>
    <ul className='relaxed flex flex-col gap-2'>{
      knownScopes(data.scopes).map((scope) => <li
        key={scope}
        className='level-left gap-2 rounded-lg bg-surface-secondary px-3 py-2 text-sm'
      >
        <LuCheck className='size-4 shrink-0 text-success' aria-hidden />
        <span>{t(scopeLabelKeys[scope])}</span>
      </li>)
    }</ul>
    <p className='relaxed text-xs opacity-60'>{t('consent.disconnectAnytime')}</p>

    <div className='flex flex-col gap-2'>
      <Button
        className='w-full'
        isPending={answering === 'allow'}
        isDisabled={answering !== null}
        onPress={() => answer('allow')}
      >
        <span>{t('consent.approve')}</span>
      </Button>
      <Button
        className='w-full'
        variant='outline'
        isPending={answering === 'deny'}
        isDisabled={answering !== null}
        onPress={() => answer('deny')}
      >
        <span>{t('consent.deny')}</span>
      </Button>
    </div>
  </div>
}
