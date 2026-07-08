// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Card, CardBody, Input, Textarea, Select, SelectItem } from '@heroui/react'

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

  function onSave() {
    console.debug('ProfileSettings save (pending backend)', { displayName, bio, status })
  }

  return <section className='container mx-auto p-6'>
    <div className='relaxed'>
      <h1 className='text-3xl'>
        <strong>{ t('auth:settings.profile.title') }</strong>
      </h1>
      <p className='opacity-80'>{ t('auth:settings.profile.subtitle') }</p>
    </div>

    <Card className='relaxed p-1'>
      <CardBody className='relaxed'>
        <Input
          label={t('auth:settings.profile.displayNameLabel')}
          value={displayName}
          onChange={(event) => setDisplayName(event.currentTarget.value)}
        />
        <Textarea
          label={t('auth:settings.profile.bioLabel')}
          value={bio}
          onChange={(event) => setBio(event.currentTarget.value)}
          minRows={3}
        />
        <Select
          label={t('auth:settings.profile.statusLabel')}
          selectedKeys={[ status ]}
          onChange={(event) => setStatus(event.target.value)}
        >
          { statusOptions.map((option) =>
            <SelectItem key={option}>{ t(`common:status.${option}`) }</SelectItem>,
          ) }
        </Select>
      </CardBody>
    </Card>

    <div className='level-right'>
      <Button color='primary' onPress={onSave}>
        <span>{ t('common:actions.save') }</span>
      </Button>
    </div>
  </section>
}
