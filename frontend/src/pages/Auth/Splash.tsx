// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Spinner } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'

// The pre-auth landing shown while the session resolves. First impression of the
// whole product, so it leads with the Elysia presence and the ambient field.
export function Splash() {
  const { t } = useTranslation([ 'auth', 'common' ])

  return <AuthShell state='idle'>
    <div className='flex flex-col items-center gap-3 text-center'>
      <h1 className='text-3xl'>
        <strong>{ t('common:appName') }</strong>
      </h1>
      <p className='opacity-80'>{ t('auth:splash.tagline') }</p>
      <div className='mt-2 flex items-center gap-2 opacity-60'>
        <Spinner size='sm' color='current' />
        <span>{ t('auth:splash.resolving') }</span>
      </div>
    </div>
  </AuthShell>
}
