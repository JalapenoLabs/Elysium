// Copyright © 2026 Jalapeno Labs

import type { ChallengeFailure } from './oauthPresentation'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { LuUnplug } from 'react-icons/lu'

type Props = {
  failure: ChallengeFailure
}

// Why connecting an app stopped. The way back is always through the app, which starts a fresh
// request, so the notice offers nothing to press.
export function ChallengeFailureNotice(props: Props) {
  const { t } = useTranslation('oauth')

  return <div className='text-center'>
    <LuUnplug className='relaxed mx-auto size-9 text-danger' aria-hidden />
    <h1 className='compact text-xl font-bold'>{t('failures.heading')}</h1>
    <p className='text-sm opacity-70'>{t(`failures.${props.failure}`)}</p>
  </div>
}
