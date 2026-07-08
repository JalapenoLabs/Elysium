// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'

// Misc
import { UrlTree } from '../../urls'

// First-run: claim an unclaimed instance by creating the root owner. On submit
// the API provisions the owner + first org in Keycloak and hands off to the
// themed enrollment (first passkey + recovery codes). The bootstrap endpoint and
// its proto contract land with the backend BFF work; this collects the details
// and orchestrates the hand-off.
export function ClaimInstance() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const [ displayName, setDisplayName ] = useState('')
  const [ email, setEmail ] = useState('')
  const [ orgName, setOrgName ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)

  function onSubmit() {
    setIsSubmitting(true)
    console.debug('ClaimInstance submit (pending backend bootstrap)', { displayName, email, orgName })
    // The real flow posts to /api/v1/auth/bootstrap, then routes into Keycloak
    // enrollment; for now it advances to onboarding.
    navigate(UrlTree.onboarding)
  }

  return <AuthShell state='idle' showPresence={false}>
    <AuthCard>
      <p className='mb-2 text-xs uppercase tracking-widest opacity-60'>
        { t('auth:setup.eyebrow') }
      </p>
      <h1 className='text-2xl'>
        <strong>{ t('auth:setup.title', { appName: t('common:appName') }) }</strong>
      </h1>
      <p className='mt-3 opacity-80'>{ t('auth:setup.subtitle') }</p>

      <div className='relaxed mt-6'>
        <Input
          autoFocus
          label={t('auth:setup.displayNameLabel')}
          placeholder={t('auth:setup.displayNamePlaceholder')}
          value={displayName}
          onChange={(event) => setDisplayName(event.currentTarget.value)}
        />
        <Input
          type='email'
          label={t('auth:setup.emailLabel')}
          placeholder={t('auth:setup.emailPlaceholder')}
          value={email}
          onChange={(event) => setEmail(event.currentTarget.value)}
        />
        <Input
          label={t('auth:setup.orgNameLabel')}
          placeholder={t('auth:setup.orgNamePlaceholder')}
          value={orgName}
          onChange={(event) => setOrgName(event.currentTarget.value)}
        />
      </div>

      <Button
        className='mt-6 w-full'
        color='primary'
        isLoading={isSubmitting}
        isDisabled={isSubmitting}
        onPress={onSubmit}
      >
        <span>{ t('auth:setup.action') }</span>
      </Button>
      <p className='mt-4 text-sm opacity-60'>{ t('auth:setup.note') }</p>
    </AuthCard>
  </AuthShell>
}
