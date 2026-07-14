// Copyright © 2026 Jalapeno Labs

// Core
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'

// Misc
import { UrlTree } from '../../urls'

// First-login welcome. Sets expectations, then hands off to credential
// enrollment (passkey + recovery codes) before returning for the profile step.
export function Welcome() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const steps = [
    t('auth:onboarding.welcome.passkeyStep'),
    t('auth:onboarding.welcome.recoveryStep'),
    t('auth:onboarding.welcome.profileStep'),
  ]

  return <AuthShell state='idle'>
    <AuthCard>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('auth:onboarding.welcome.eyebrow') }
      </p>
      <h1 className='text-2xl'>
        <strong>{ t('auth:onboarding.welcome.title', { appName: t('common:appName') }) }</strong>
      </h1>
      <p className='mt-3 opacity-80'>{ t('auth:onboarding.welcome.body') }</p>

      <ol className='relaxed mt-6'>
        { steps.map((step, index) =>
          <li key={step} className='flex items-center gap-3'>
            <span className='flex h-7 w-7 items-center justify-center rounded-full bg-primary/15 text-sm text-primary'>
              { index + 1 }
            </span>
            <span>{ step }</span>
          </li>,
        ) }
      </ol>

      <Button
        className='mt-6 w-full'
        color='primary'
        onPress={() => navigate(UrlTree.onboardingProfile)}
      >
        <span>{ t('auth:onboarding.welcome.action') }</span>
      </Button>
    </AuthCard>
  </AuthShell>
}
