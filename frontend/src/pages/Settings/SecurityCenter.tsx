// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Button, Card, CardBody, Chip, Tooltip } from '@heroui/react'
import { LoginHistory } from './LoginHistory'
import { AddAuthenticatorModal } from './AddAuthenticatorModal'

// Utility
import moment from 'moment-timezone'

// Misc
import { enrollPasskey } from '../../api/routes/authRoutes'
import { getCredentials, type CredentialsSummary } from '../../api/routes/userRoutes'

type SecuritySectionProps = {
  title: string
  body: string
  actionLabel: string
  // A live section provides `onAction`; the rest stay disabled with the tooltip
  // explaining why until their endpoints ship (see docs/auth-model.md).
  onAction?: () => void
  isBusy?: boolean
  statusLine?: { tone: 'success' | 'danger', message: string } | null
  // An enrolled/status chip beside the title.
  status?: { label: string, tone: 'success' | 'default' } | null
  disabledReason?: string
  // Detail rendered under the header (e.g. the list of enrolled passkeys).
  children?: ReactNode
}

// One security area: a title, a short description, its primary action, and an
// optional detail body (the enrolled credentials).
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
    <CardBody>
      <div className='level items-start'>
        <div>
          <div className='flex items-center gap-2'>
            <h2 className='text-lg'>
              <strong>{ props.title }</strong>
            </h2>
            { props.status
              ? <Chip
                size='sm'
                variant='flat'
                color={props.status.tone === 'success' ? 'success' : 'default'}
              >
                { props.status.label }
              </Chip>
              : null }
          </div>
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
      </div>
      { props.children
        ? <div className='mt-4'>{ props.children }</div>
        : null }
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
  const [ isAuthenticatorOpen, setIsAuthenticatorOpen ] = useState(false)
  const [ credentials, setCredentials ] = useState<CredentialsSummary | null>(null)

  const comingSoon = t('settings.security.comingSoon')

  // The account's enrolled credentials, reloaded after any enrollment so the
  // lists reflect what was just added.
  async function loadCredentials() {
    try {
      setCredentials(await getCredentials())
    }
    catch (error) {
      console.debug('credentials unavailable', { error })
    }
  }

  useEffect(() => {
    loadCredentials()
  }, [])

  // Adds another passkey to the signed-in account: the native registration
  // ceremony against the authenticated add-credential endpoints. This is also
  // the landing action after a recovery-code sign-in.
  async function onAddPasskey() {
    setIsEnrolling(true)
    setEnrollOutcome(null)
    try {
      await enrollPasskey()
      setEnrollOutcome({ tone: 'success', message: t('settings.security.passkeyAdded') })
      await loadCredentials()
    }
    catch (error) {
      console.debug('passkey enrollment failed', { error })
      setEnrollOutcome({ tone: 'danger', message: t('settings.security.passkeyFailed') })
    }
    finally {
      setIsEnrolling(false)
    }
  }

  const passkeys = credentials?.passkeys ?? []
  const totpEnrolled = credentials?.totp.enrolled ?? false
  const passwordEnrolled = credentials?.password.enrolled ?? false

  return <section className='w-full'>
    <AddAuthenticatorModal
      isOpen={isAuthenticatorOpen}
      onOpenChange={setIsAuthenticatorOpen}
      onEnrolled={loadCredentials}
    />

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
      >
        { passkeys.length
          ? <div className='flex flex-col gap-2'>
            { passkeys.map((passkey) =>
              <div key={passkey.id} className='level items-center rounded-medium bg-default-100 px-3 py-2'>
                <span className='text-sm'>
                  { t('settings.security.passkeyAddedOn', {
                    when: moment(passkey.created_at).format('MMM D, YYYY'),
                  }) }
                </span>
                <span className='text-xs opacity-60'>
                  { passkey.last_used_at
                    ? t('settings.security.passkeyLastUsed', {
                      when: moment(passkey.last_used_at).fromNow(),
                    })
                    : t('settings.security.passkeyNeverUsed') }
                </span>
              </div>,
            ) }
          </div>
          : credentials
            ? <p className='text-sm opacity-60'>{ t('settings.security.passkeysEmpty') }</p>
            : null }
      </SecuritySection>
    </div>
    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.authenticatorsTitle')}
        body={t('settings.security.authenticatorsBody')}
        actionLabel={t('settings.security.addAuthenticator')}
        onAction={() => setIsAuthenticatorOpen(true)}
        status={totpEnrolled
          ? { label: t('settings.security.enrolled'), tone: 'success' }
          : null}
      />
    </div>
    <div className='relaxed'>
      <SecuritySection
        title={t('settings.security.passwordTitle')}
        body={t('settings.security.passwordBody')}
        actionLabel={t('settings.security.manage')}
        status={passwordEnrolled
          ? { label: t('settings.security.enrolled'), tone: 'success' }
          : { label: t('settings.security.notSet'), tone: 'default' }}
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
    <div className='relaxed'>
      <LoginHistory />
    </div>
  </section>
}
