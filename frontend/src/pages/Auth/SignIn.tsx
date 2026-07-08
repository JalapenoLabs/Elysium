// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'

// Misc
import { useSession } from '../../auth/useSession'

// The anonymous entry point. "Sign in" is a full-page navigation to the gateway
// login initiator; the passkey/TOTP ceremony itself is Keycloak's themed flow.
export function SignIn() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const session = useSession()

  return <AuthShell state='needsInput'>
    <AuthCard>
      <h1 className='text-2xl'>
        <strong>{ t('auth:signIn.title') }</strong>
      </h1>
      <p className='mt-3 opacity-80'>
        { t('auth:signIn.subtitle', { appName: t('common:appName') }) }
      </p>
      <Button
        className='mt-6 w-full'
        color='primary'
        onPress={session.signIn}
      >
        <span>{ t('auth:signIn.action') }</span>
      </Button>
    </AuthCard>
  </AuthShell>
}
