// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useNavigate } from 'react-router'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Input, Textarea, Select, SelectItem } from '@heroui/react'
import { AuthShell } from '../../components/auth/AuthShell'
import { AuthCard } from '../../components/auth/AuthCard'

// Misc
import { UrlTree } from '../../urls'

// The user-status options mirror `elysium.user.v1.UserStatus`. The values are
// machine identifiers; only the labels are translated (see ~/.claude/docs/i18n.md).
const statusOptions = [ 'online', 'away', 'offline', 'outOfOffice' ] as const

// Final onboarding step: the app-owned profile (name, bio, status). Credential
// enrollment happened in Keycloak's themed flow just before this.
export function ProfileStep() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const navigate = useNavigate()

  const [ displayName, setDisplayName ] = useState('')
  const [ bio, setBio ] = useState('')
  const [ status, setStatus ] = useState<string>('online')

  function onFinish() {
    console.debug('ProfileStep submit (pending backend)', { displayName, bio, status })
    navigate(UrlTree.home)
  }

  return <AuthShell state='idle' showPresence={false}>
    <AuthCard>
      <h1 className='text-2xl'>
        <strong>{ t('auth:onboarding.profile.title') }</strong>
      </h1>
      <p className='mt-3 opacity-80'>{ t('auth:onboarding.profile.subtitle') }</p>

      <div className='relaxed mt-6'>
        <Input
          autoFocus
          label={t('auth:onboarding.profile.displayNameLabel')}
          placeholder={t('auth:onboarding.profile.displayNamePlaceholder')}
          value={displayName}
          onChange={(event) => setDisplayName(event.currentTarget.value)}
        />
        <Textarea
          label={t('auth:onboarding.profile.bioLabel')}
          placeholder={t('auth:onboarding.profile.bioPlaceholder')}
          value={bio}
          onChange={(event) => setBio(event.currentTarget.value)}
          minRows={3}
        />
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

      <Button
        className='mt-6 w-full'
        color='primary'
        onPress={onFinish}
      >
        <span>{ t('auth:onboarding.profile.action') }</span>
      </Button>
    </AuthCard>
  </AuthShell>
}
