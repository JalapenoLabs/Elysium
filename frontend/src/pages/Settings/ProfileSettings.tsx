// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Card, CardBody, Input, Textarea, Select, SelectItem } from '@heroui/react'
import { SubmitButton } from '../../components/form/SubmitButton'

// Misc
import { useSession } from '../../auth/useSession'

// User-status options mirror `elysium.user.v1.UserStatus`; only labels translate.
const statusOptions = [ 'online', 'away', 'offline', 'outOfOffice' ] as const

// The account profile: name, bio, and status. Saving persists via the profile
// endpoint (pending backend); the values seed from the resolved session.
export function ProfileSettings() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const session = useSession()

  const [ displayName, setDisplayName ] = useState(session.principal?.username ?? '')
  const [ bio, setBio ] = useState('')
  const [ status, setStatus ] = useState<string>('online')

  // Focus the first field on mount.
  const displayNameRef = useRef<HTMLInputElement>(null)
  useEffect(() => {
    displayNameRef.current?.focus()
  }, [])

  const canSubmit = Boolean(displayName.trim())

  function onSave(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    console.debug('ProfileSettings save (pending backend)', { displayName, bio, status })
  }

  return <section className='container mx-auto p-6'>
    <div className='relaxed'>
      <h1 className='text-3xl'>
        <strong>{ t('auth:settings.profile.title') }</strong>
      </h1>
      <p className='opacity-80'>{ t('auth:settings.profile.subtitle') }</p>
    </div>

    <form onSubmit={onSave}>
      <Card className='p-1'>
        <CardBody>
          <div className='compact'>
            <Input
              ref={displayNameRef}
              label={t('auth:settings.profile.displayNameLabel')}
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
          <div className='compact'>
            <Select
              label={t('auth:settings.profile.statusLabel')}
              selectedKeys={[ status ]}
              onChange={(event) => setStatus(event.target.value)}
            >
              { statusOptions.map((option) =>
                <SelectItem key={option}>{ t(`common:status.${option}`) }</SelectItem>,
              ) }
            </Select>
          </div>
        </CardBody>
      </Card>

      <div className='level-right mt-6'>
        <SubmitButton
          fullWidth={false}
          label={t('common:actions.save')}
          isValid={canSubmit}
          invalidReason={t('auth:settings.profile.required')}
        />
      </div>
    </form>
  </section>
}
