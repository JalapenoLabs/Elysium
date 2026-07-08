// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { Navigate, useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Button, Input } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'

// Misc
import { signInWithPasskey } from '../../api/routes/authRoutes'
import { resolveSession } from '../../auth/session'
import { UrlTree } from '../../urls'

// The anonymous entry point. The user enters their email and signs in with a
// passkey: the ceremony runs natively in the browser against our API (the WebAuthn
// Relying Party), never a redirect to an identity provider. See docs/auth-model.md.
export function SignIn() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()
  const instanceClaimed = useAppSelector((state) => state.session.instanceClaimed)

  const [ email, setEmail ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  // An unclaimed instance has no accounts to sign in to: send the first user to
  // the first-run claim screen instead.
  if (instanceClaimed === false) {
    return <Navigate to={UrlTree.setup} replace />
  }

  async function onSubmit() {
    if (!email.trim()) {
      setErrorMessage(t('auth:signIn.emailRequired'))
      return
    }

    setIsSubmitting(true)
    setErrorMessage(null)
    try {
      await signInWithPasskey(email.trim())
      await resolveSession()
      navigate(UrlTree.home)
    }
    catch (error) {
      console.debug('passkey sign-in failed', { error })
      setErrorMessage(t('auth:signIn.failed'))
    }
    finally {
      setIsSubmitting(false)
    }
  }

  return <AuthShell state='needsInput'>
    <AuthCard>
      <h1 className='text-2xl'>
        <strong>{ t('auth:signIn.title') }</strong>
      </h1>
      <p className='mt-3 opacity-80'>
        { t('auth:signIn.subtitle', { appName: t('common:appName') }) }
      </p>

      <div className='mt-6'>
        <Input
          autoFocus
          type='email'
          label={t('auth:signIn.emailLabel')}
          placeholder={t('auth:signIn.emailPlaceholder')}
          value={email}
          isInvalid={Boolean(errorMessage)}
          errorMessage={errorMessage}
          onChange={(event) => setEmail(event.currentTarget.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') {
              void onSubmit()
            }
          }}
        />
      </div>

      <Button
        className='mt-6 w-full'
        color='primary'
        isLoading={isSubmitting}
        isDisabled={isSubmitting}
        onPress={onSubmit}
      >
        <span>{ t('auth:signIn.action') }</span>
      </Button>
    </AuthCard>
  </AuthShell>
}
