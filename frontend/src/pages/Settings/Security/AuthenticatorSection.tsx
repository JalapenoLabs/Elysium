// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from './useSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Chip } from '@heroui/react'
import { AuthenticatorCodeInput } from '../../../components/AuthenticatorCodeInput'
import { SecuritySection } from './SecuritySection'

// Misc
import { collectMessages } from '../../../api/kratosFlows'
import { useConfirm } from '../../../hooks/useConfirm'
import { fieldErrorText } from '../../Auth/kratosPresentation'
import { readAuthenticator } from './securityPresentation'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// An authenticator app's six-digit code, asked for after every password or passkey sign-in
// once it is set up.
export function AuthenticatorSection(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const [ code, setCode ] = useState('')
  const [ isSaving, setIsSaving ] = useState(false)
  const authenticator = readAuthenticator(props.flow)
  const messages = collectMessages(props.flow)
  const isEnrolled = authenticator.kind === 'enrolled'

  async function enroll(totpCode: string) {
    setIsSaving(true)
    await props.onSubmit({ method: 'totp', totp_code: totpCode }, t('security.authenticator.enrolled'))
    setCode('')
    setIsSaving(false)
  }

  function unlink() {
    confirm({
      title: t('security.authenticator.removeTitle'),
      message: t('security.authenticator.removeMessage'),
      tone: 'danger',
      confirmText: t('security.authenticator.remove'),
      onConfirm: async () => {
        await props.onSubmit({ method: 'totp', totp_unlink: true }, t('security.authenticator.removed'))
      },
    })
  }

  return <SecuritySection
    title={t('security.authenticator.title')}
    description={t('security.authenticator.description')}
    status={<Chip size='sm' variant='soft' color={isEnrolled
      ? 'success'
      : 'default'}>{
        isEnrolled
          ? t('security.authenticator.on')
          : t('security.authenticator.off')
      }</Chip>}
  >
    {isEnrolled && <Button size='sm' variant='outline' onPress={unlink}>
      <span>{t('security.authenticator.remove')}</span>
    </Button>}

    {authenticator.kind === 'available' && <div className='flex flex-col gap-6 md:flex-row md:items-start'>
      <img
        src={authenticator.qrCode}
        alt={t('security.authenticator.qrAlt')}
        className='size-44 shrink-0 rounded-lg bg-white p-2'
      />
      <div className='min-w-0'>
        <p className='compact text-sm'>{t('security.authenticator.scan')}</p>
        <p className='compact text-xs opacity-70'>{t('security.authenticator.manualKey')}</p>
        <code className='relaxed block break-all rounded-md bg-surface-secondary px-3 py-2 text-sm'>{
          authenticator.secretKey
        }</code>
        <p className='compact text-sm'>{t('security.authenticator.enterCode')}</p>
        <AuthenticatorCodeInput
          label={t('fields.authenticatorCode')}
          value={code}
          isDisabled={isSaving}
          isInvalid={Boolean(messages.byField.totp_code)}
          onChange={setCode}
          onComplete={enroll}
        />
        {messages.byField.totp_code && <p className='mt-2 text-xs text-danger'>{
          fieldErrorText(messages.byField.totp_code, t)
        }</p>}
      </div>
    </div>}
  </SecuritySection>
}
