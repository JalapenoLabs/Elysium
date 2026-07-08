// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Input, Textarea, Select, SelectItem } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'
import { SubmitButton } from '../../components/form/SubmitButton'

// Misc
import { UrlTree } from '../../urls'

// The user-status options mirror `elysium.user.v1.UserStatus`. The values are
// machine identifiers; only the labels are translated (see ~/.claude/docs/i18n.md).
const statusOptions = [ 'online', 'away', 'offline', 'outOfOffice' ] as const

// Final onboarding step: the app-owned profile (name, bio, status).
export function ProfileStep() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const [ displayName, setDisplayName ] = useState('')
  const [ bio, setBio ] = useState('')
  const [ status, setStatus ] = useState<string>('online')

  // Focus the first field on mount; autoFocus is unreliable with the card's
  // entrance animation and StrictMode's double-mount in dev.
  const displayNameRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    displayNameRef.current?.focus()
  }, [])

  const canSubmit = Boolean(displayName.trim())

  function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    console.debug('ProfileStep submit (pending backend)', { displayName, bio, status })
    navigate(UrlTree.home)
  }

  return <AuthShell state='idle' showPresence={false}>
    <AuthCard>
      <h1 className='text-2xl'>
        <strong>{ t('auth:onboarding.profile.title') }</strong>
      </h1>
      <p className='mt-3 opacity-80'>{ t('auth:onboarding.profile.subtitle') }</p>

      <form onSubmit={onSubmit}>
        <div className='mt-6'>
          <div className='compact'>
            <Input
              ref={displayNameRef}
              label={t('auth:onboarding.profile.displayNameLabel')}
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
          <div className='compact'>
            <Select
              label={t('auth:onboarding.profile.statusLabel')}
              selectedKeys={[ status ]}
              onChange={(event) => setStatus(event.target.value)}
            >
              { statusOptions.map((option) =>
                <SelectItem key={option}>{ t(`common:status.${option}`) }</SelectItem>,
              ) }
            </Select>
          </div>
        </div>

        <SubmitButton
          className='mt-6'
          label={t('auth:onboarding.profile.action')}
          isValid={canSubmit}
          invalidReason={t('auth:onboarding.profile.required')}
        />
      </form>
    </AuthCard>
  </AuthShell>
}
