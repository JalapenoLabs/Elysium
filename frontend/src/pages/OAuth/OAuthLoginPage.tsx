// Copyright © 2026 Jalapeno Labs

import type { ChallengeFailure } from './oauthPresentation'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useSearchParams } from 'react-router'

// User interface
import { Spinner } from '@heroui/react'
import { ChallengeFailureNotice } from './ChallengeFailureNotice'

// Misc
import { acceptOAuthLogin } from '../../api/routes/oauthRoutes'
import { challengeFailure } from './oauthPresentation'

// Where Hydra sends someone connecting an MCP client to sign in. They are already signed in here
// (the auth gate saw to that), so the page answers at once, as them, and moves on to consent.
export function OAuthLoginPage() {
  const { t } = useTranslation('oauth')
  const [ params ] = useSearchParams()
  const challenge = params.get('login_challenge')
  const [ failure, setFailure ] = useState<ChallengeFailure | null>(challenge
    ? null
    : 'expired')
  // A challenge answers once; StrictMode's second effect run must not answer it again.
  const answered = useRef(false)

  useEffect(() => {
    if (!challenge || answered.current) {
      return
    }
    answered.current = true

    acceptOAuthLogin(challenge)
      .then((response) => window.location.assign(response.redirectTo))
      .catch((error: unknown) => {
        console.debug('OAuthLoginPage could not answer the sign-in', { error })
        setFailure(challengeFailure(error))
      })
  }, [ challenge ])

  if (failure) {
    return <ChallengeFailureNotice failure={failure} />
  }

  return <div className='grid place-items-center gap-4 py-8 text-center'>
    <Spinner />
    <p className='text-sm opacity-70'>{t('login.connecting')}</p>
  </div>
}
