// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Button, Card, CardBody, Chip } from '@heroui/react'

// Misc
import { useSession } from '../../auth/useSession'

// Where the account is signed in. The current device is always shown; the list
// of other sessions and remote sign-out land with the sessions endpoint.
export function ActiveSessions() {
  const { t } = useTranslation('auth')
  const session = useSession()

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('settings.sessions.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('settings.sessions.subtitle') }</p>
    </div>

    <div className='relaxed'>
      <Card className='p-1'>
        <CardBody className='level items-center'>
          <div>
            <div className='flex items-center gap-2'>
              <strong>{ session.principal?.username || session.principal?.email }</strong>
              <Chip size='sm' variant='flat' color='success'>
                { t('settings.sessions.thisDevice') }
              </Chip>
            </div>
          </div>
          <Button variant='flat' isDisabled>
            <span>{ t('settings.sessions.signOut') }</span>
          </Button>
        </CardBody>
      </Card>
    </div>

    <div className='relaxed'>
      <p className='opacity-60'>{ t('settings.sessions.empty') }</p>
    </div>
  </section>
}
