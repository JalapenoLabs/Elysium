// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'

// Misc
import { bootstrapPasskey } from '../../api/routes/authRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

// First-run: claim an unclaimed instance by creating the root owner. On submit the
// API creates the owner + first org and starts a passkey registration; the browser
// runs the native `navigator.credentials.create` ceremony and the API stores the
// passkey and signs the owner in. No identity provider, no redirect. See
// docs/auth-model.md.
export function ClaimInstance() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const [ displayName, setDisplayName ] = useState('')
  const [ email, setEmail ] = useState('')
  const [ orgName, setOrgName ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    if (!displayName.trim() || !email.trim()) {
      setErrorMessage(t('auth:setup.required'))
      return
    }

    setIsSubmitting(true)
    setErrorMessage(null)
    try {
      await bootstrapPasskey(email.trim(), displayName.trim(), orgName.trim())
      await resolveSession()
      navigate(UrlTree.onboarding)
    }
    catch (error) {
      console.debug('instance claim failed', { error })
      setErrorMessage(t('auth:setup.failed'))
      setIsSubmitting(false)
    }
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

      <form onSubmit={onSubmit}>
        <div className='mt-6'>
          <div className='compact'>
            <Input
              autoFocus
              label={t('auth:setup.displayNameLabel')}
              value={displayName}
              onChange={(event) => setDisplayName(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Input
              type='email'
              label={t('auth:setup.emailLabel')}
              value={email}
              onChange={(event) => setEmail(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Input
              label={t('auth:setup.orgNameLabel')}
              value={orgName}
              onChange={(event) => setOrgName(event.currentTarget.value)}
            />
          </div>
        </div>

        { errorMessage
          ? <p className='mt-3 text-sm text-danger'>{ errorMessage }</p>
          : null }

        <Button
          type='submit'
          className='mt-6 w-full'
          color='primary'
          isLoading={isSubmitting}
          isDisabled={isSubmitting}
        >
          <span>{ t('auth:setup.action') }</span>
        </Button>
      </form>
    </AuthCard>
  </AuthShell>
}
