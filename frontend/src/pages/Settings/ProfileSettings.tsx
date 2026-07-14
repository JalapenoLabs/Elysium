// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Card, CardBody, Input, Textarea } from '@heroui/react'
import { SubmitButton } from '../../components/form/SubmitButton'

// Misc
import { useCurrentUser } from '../../auth/useCurrentUser'
import { updateMyProfile } from '../../api/routes/userRoutes'

// The account profile: the split name model (given/family are the full name,
// display name is the render field with a server-side given-name fallback) and
// the bio. Presence is derived automatically and the manual override lives in
// the navbar status control, so there is no status field here.
export function ProfileSettings() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const user = useCurrentUser()

  const [ givenName, setGivenName ] = useState('')
  const [ familyName, setFamilyName ] = useState('')
  const [ displayName, setDisplayName ] = useState('')
  const [ bio, setBio ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const [ isSaved, setIsSaved ] = useState(false)
  const [ errorMessage, setErrorMessage ] = useState<string | null>(null)

  // Seed the form when the profile arrives; anything already typed wins so a
  // slow load never clobbers the user's edits.
  useEffect(() => {
    if (!user) {
      return
    }
    setGivenName((current) => current || user.givenName)
    setFamilyName((current) => current || user.familyName)
    setDisplayName((current) => current || user.displayName)
    setBio((current) => current || user.bio)
  }, [ user ])

  // Focus the first field on mount.
  const givenNameRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    givenNameRef.current?.focus()
  }, [])

  const canSubmit = Boolean(givenName.trim())

  async function onSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()

    setIsSubmitting(true)
    setIsSaved(false)
    setErrorMessage(null)
    try {
      const updated = await updateMyProfile({
        givenName: givenName.trim(),
        familyName: familyName.trim(),
        displayName: displayName.trim(),
        bio: bio.trim(),
      })
      // Reflect server-side normalization (e.g. the display-name fallback).
      setDisplayName(updated.displayName)
      setIsSaved(true)
    }
    catch (error) {
      console.debug('profile save failed', { error })
      setErrorMessage(t('auth:settings.profile.failed'))
    }
    finally {
      setIsSubmitting(false)
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('auth:settings.profile.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('auth:settings.profile.subtitle') }</p>
    </div>

    <form onSubmit={onSave}>
      <Card className='p-1'>
        <CardBody>
          <div className='compact level items-start'>
            <Input
              ref={givenNameRef}
              className='w-full'
              label={t('auth:settings.profile.givenNameLabel')}
              value={givenName}
              onChange={(event) => setGivenName(event.currentTarget.value)}
            />
            <Input
              className='w-full'
              label={t('auth:settings.profile.familyNameLabel')}
              value={familyName}
              onChange={(event) => setFamilyName(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Input
              label={t('auth:settings.profile.displayNameLabel')}
              description={t('auth:settings.profile.displayNameHint')}
              value={displayName}
              onChange={(event) => setDisplayName(event.currentTarget.value)}
            />
          </div>
          <div className='compact'>
            <Textarea
              label={t('auth:settings.profile.bioLabel')}
              value={bio}
              onChange={(event) => setBio(event.currentTarget.value)}
              minRows={3}
            />
          </div>
        </CardBody>
      </Card>

      <div className='level mt-6'>
        <div>
          { errorMessage
            ? <p className='text-sm text-danger'>{ errorMessage }</p>
            : null }
          { isSaved
            ? <p className='text-sm text-success'>{ t('auth:settings.profile.saved') }</p>
            : null }
        </div>
        <SubmitButton
          fullWidth={false}
          label={t('common:actions.save')}
          isValid={canSubmit}
          invalidReason={t('auth:settings.profile.required')}
          isSubmitting={isSubmitting}
        />
      </div>
    </form>
  </section>
}
