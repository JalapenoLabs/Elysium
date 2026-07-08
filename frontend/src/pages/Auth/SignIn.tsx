// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { Navigate, useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppSelector } from '../../store/hooks'

// UI
import { Input } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { SubmitButton } from '../../components/form/SubmitButton'

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

  // Focus the email field on mount. `autoFocus` is unreliable here because the card
  // animates in (and StrictMode double-mounts in dev), so focus imperatively.
  const emailRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    emailRef.current?.focus()
  }, [])

  // An unclaimed instance has no accounts to sign in to: send the first user to
  // the first-run claim screen instead.
  if (instanceClaimed === false) {
    return <Navigate to={UrlTree.setup} replace />
  }

  // The submit button stays disabled until an email is entered, so the form never
  // flags its own pristine empty state; the error line is for an actual failure.
  const canSubmit = Boolean(email.trim())

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

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

      <form onSubmit={onSubmit}>
        <div className='mt-6'>
          <Input
            ref={emailRef}
            type='email'
            label={t('auth:signIn.emailLabel')}
            value={email}
            onChange={(event) => setEmail(event.currentTarget.value)}
          />
        </div>

        { errorMessage
          ? <p className='mt-3 text-sm text-danger'>{ errorMessage }</p>
          : null }

        <SubmitButton
          className='mt-6'
          label={t('auth:signIn.action')}
          isValid={canSubmit}
          invalidReason={t('auth:signIn.emailRequired')}
          isSubmitting={isSubmitting}
        />
      </form>
    </AuthCard>
  </AuthShell>
}
