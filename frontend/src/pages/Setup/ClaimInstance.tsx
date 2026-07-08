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

  async function onSubmit() {
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
          isInvalid={Boolean(errorMessage)}
          errorMessage={errorMessage}
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
