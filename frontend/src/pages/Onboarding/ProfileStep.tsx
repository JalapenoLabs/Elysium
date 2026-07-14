// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Input, Textarea } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { SubmitButton } from '../../components/form/SubmitButton'

// Misc
import { getMyProfile, updateMyProfile } from '../../api/routes/userRoutes'
import { UrlTree } from '../../urls'

// Final onboarding step: the app-owned profile. Names follow the split model
// (see docs/identity.md): given/family are the full name, display name is the
// single render field and falls back server-side to the given name when blank.
// Presence is derived automatically, so there is deliberately no status picker
// here; the manual override lives in the app chrome.
export function ProfileStep() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const [ givenName, setGivenName ] = useState('')
  const [ familyName, setFamilyName ] = useState('')
  const [ displayName, setDisplayName ] = useState('')
  const [ bio, setBio ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  // The claim / invite step may have collected names already, so prefill from
  // the server and let the user confirm or refine instead of retyping. Fields
  // the user has already started editing are never clobbered by a slow prefill.
  useEffect(() => {
    let cancelled = false
    getMyProfile()
      .then((profile) => {
        if (cancelled) {
          return
        }
        setGivenName((current) => current || profile.givenName)
        setFamilyName((current) => current || profile.familyName)
        setDisplayName((current) => current || profile.displayName)
        setBio((current) => current || profile.bio)
      })
      .catch((error) => {
        console.debug('profile prefill unavailable', { error })
      })
    return () => {
      cancelled = true
    }
  }, [])

  // Focus the first field on mount; autoFocus is unreliable with the card's
  // entrance animation and StrictMode's double-mount in dev.
  const givenNameRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    givenNameRef.current?.focus()
  }, [])

  // The given name is the only requirement: display name falls back to it
  // server-side and not everyone has (or wants to share) a family name.
  const canSubmit = Boolean(givenName.trim())

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    setIsSubmitting(true)
    setErrorMessage(null)
    try {
      await updateMyProfile({
        givenName: givenName.trim(),
        familyName: familyName.trim(),
        displayName: displayName.trim(),
        bio: bio.trim(),
      })
      navigate(UrlTree.home)
    }
    catch (error) {
      console.debug('profile update failed', { error })
      setErrorMessage(t('auth:onboarding.profile.failed'))
    }
    finally {
      setIsSubmitting(false)
    }
  }

  return <AuthShell state='idle' showPresence={false}>
    <AuthCard>
      <h1 className='text-2xl'>
        <strong>{ t('auth:onboarding.profile.title') }</strong>
      </h1>
      <p className='mt-3 opacity-80'>{ t('auth:onboarding.profile.subtitle') }</p>

      <form onSubmit={onSubmit}>
        <div className='mt-6'>
          <div className='compact level items-start'>
            <Input
              ref={givenNameRef}
              className='w-full'
              label={t('auth:onboarding.profile.givenNameLabel')}
              value={givenName}
              onChange={(event) => setGivenName(event.currentTarget.value)}
            />
            <Input
              className='w-full'
              label={t('auth:onboarding.profile.familyNameLabel')}
              value={familyName}
              onChange={(event) => setFamilyName(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Input
              label={t('auth:onboarding.profile.displayNameLabel')}
              description={t('auth:onboarding.profile.displayNameHint')}
              value={displayName}
              onChange={(event) => setDisplayName(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Textarea
              label={t('auth:onboarding.profile.bioLabel')}
              value={bio}
              onChange={(event) => setBio(event.currentTarget.value)}
              minRows={3}
            />
          </div>
        </div>

        { errorMessage
          ? <p className='mt-3 text-sm text-danger'>{ errorMessage }</p>
          : null }

        <SubmitButton
          className='mt-6'
          label={t('auth:onboarding.profile.action')}
          isValid={canSubmit}
          invalidReason={t('auth:onboarding.profile.required')}
          isSubmitting={isSubmitting}
        />
      </form>
    </AuthCard>
  </AuthShell>
}
