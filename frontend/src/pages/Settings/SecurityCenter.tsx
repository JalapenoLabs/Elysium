// Copyright © 2026 Jalapeno Labs

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Card, CardBody, Tooltip } from '@heroui/react'

// Misc
import { enrollPasskey } from '../../api/routes/authRoutes'

type SecuritySectionProps = {
  title: string
  body: string
  actionLabel: string
  // A live section provides `onAction`; the rest stay disabled with the tooltip
  // explaining why until their endpoints ship (see docs/auth-model.md).
  onAction?: () => void
  isBusy?: boolean
  statusLine?: { tone: 'success' | 'danger', message: string } | null
  disabledReason?: string
}

// One security area: a title, a short description, and its primary action.
function SecuritySection(props: SecuritySectionProps) {
  const action = <Button
    variant='flat'
    isDisabled={!props.onAction || props.isBusy}
    isLoading={props.isBusy}
    onPress={props.onAction}
  >
    <span>{ props.actionLabel }</span>
  </Button>

  return <Card className='p-1'>
    <CardBody className='level items-start'>
      <div>
        <h2 className='text-lg'>
          <strong>{ props.title }</strong>
        </h2>
        <p className='mt-1 opacity-70'>{ props.body }</p>
        { props.statusLine
          ? <p className={`mt-1 text-sm text-${props.statusLine.tone}`}>
            { props.statusLine.message }
          </p>
          : null }
      </div>
      { props.onAction
        ? action
        : <Tooltip content={props.disabledReason} placement='top'>
          {/* Disabled buttons fire no hover events, so the tooltip needs a div. */}
          <div>{ action }</div>
        </Tooltip>
      }
    </CardBody>
  </Card>
}

// The identity home: manage passkeys, authenticator apps, and recovery codes.
export function SecurityCenter() {
  const { t } = useTranslation('auth')

  const [ isEnrolling, setIsEnrolling ] = useState(false)
  const [ enrollOutcome, setEnrollOutcome ] = useState<
    { tone: 'success' | 'danger', message: string } | null
  >(null)

  const comingSoon = t('settings.security.comingSoon')

  // Adds another passkey to the signed-in account: the native registration
  // ceremony against the authenticated add-credential endpoints. This is also
  // the landing action after a recovery-code sign-in.
  async function onAddPasskey() {
    setIsEnrolling(true)
    setEnrollOutcome(null)
    try {
      await enrollPasskey()
      setEnrollOutcome({ tone: 'success', message: t('settings.security.passkeyAdded') })
    }
    catch (error) {
      console.debug('passkey enrollment failed', { error })
      setEnrollOutcome({ tone: 'danger', message: t('settings.security.passkeyFailed') })
    }
    finally {
      setIsEnrolling(false)
    }
  }

  return <section className='w-full'>
    <div className='relaxed'>
      <h2 className='text-2xl'>
        <strong>{ t('settings.security.title') }</strong>
      </h2>
      <p className='opacity-80'>{ t('settings.security.subtitle') }</p>
    </div>

    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.passkeysTitle')}
        body={t('settings.security.passkeysBody')}
        actionLabel={t('settings.security.addPasskey')}
        onAction={onAddPasskey}
        isBusy={isEnrolling}
        statusLine={enrollOutcome}
      />
    </div>
    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.authenticatorsTitle')}
        body={t('settings.security.authenticatorsBody')}
        actionLabel={t('settings.security.manage')}
        disabledReason={comingSoon}
      />
    </div>
    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.recoveryTitle')}
        body={t('settings.security.recoveryBody')}
        actionLabel={t('settings.security.manage')}
        disabledReason={comingSoon}
      />
    </div>
  </section>
}
