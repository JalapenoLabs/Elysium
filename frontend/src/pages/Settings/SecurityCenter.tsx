// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// UI
import { Button, Card, CardBody } from '@heroui/react'

// Misc
import { API_BASE_URL } from '../../constants'

type SecuritySectionProps = {
  title: string
  body: string
  actionLabel: string
}

// One security area: a title, a short description, and an action that deep-links
// into Keycloak's themed account flow (credential changes are Keycloak's, not
// reimplemented here). Local to this page.
function SecuritySection(props: SecuritySectionProps) {
  return <Card className='p-1'>
    <CardBody className='level items-start'>
      <div>
        <h2 className='text-lg'>
          <strong>{ props.title }</strong>
        </h2>
        <p className='mt-1 opacity-70'>{ props.body }</p>
      </div>
      <Button
        variant='flat'
        onPress={() => window.location.assign(`${API_BASE_URL}/auth/account`)}
      >
        <span>{ props.actionLabel }</span>
      </Button>
    </CardBody>
  </Card>
}

// The identity home: manage passkeys, authenticator apps, and recovery codes.
export function SecurityCenter() {
  const { t } = useTranslation('auth')

  return <section className='container mx-auto p-6'>
    <div className='relaxed'>
      <h1 className='text-3xl'>
        <strong>{ t('settings.security.title') }</strong>
      </h1>
      <p className='opacity-80'>{ t('settings.security.subtitle') }</p>
    </div>

    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.passkeysTitle')}
        body={t('settings.security.passkeysBody')}
        actionLabel={t('settings.security.addPasskey')}
      />
    </div>
    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.authenticatorsTitle')}
        body={t('settings.security.authenticatorsBody')}
        actionLabel={t('settings.security.manage')}
      />
    </div>
    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.recoveryTitle')}
        body={t('settings.security.recoveryBody')}
        actionLabel={t('settings.security.manage')}
      />
    </div>
  </section>
}
