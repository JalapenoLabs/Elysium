// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Spinner } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'

// The transient handoff after the ceremony completes, while the app hydrates the
// session and moves the user into the workspace. Cinematic, not a white flash.
export function SigningIn() {
  const { t } = useTranslation([ 'auth', 'common' ])

  return <AuthShell state='authenticating'>
    <div className='flex flex-col items-center gap-3 text-center'>
      <h1 className='text-2xl'>
        <strong>{ t('auth:signingIn.title') }</strong>
      </h1>
      <p className='opacity-80'>
        { t('auth:signingIn.subtitle', { aiName: t('common:aiName') }) }
      </p>
      <Spinner className='mt-2' color='current' />
    </div>
  </AuthShell>
}
