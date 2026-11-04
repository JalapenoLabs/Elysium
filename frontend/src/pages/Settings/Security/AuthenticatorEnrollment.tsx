// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Form, Input, Label, Modal, Spinner, TextField, toast } from '@heroui/react'
import { AuthenticatorCodeInput } from '../../../components/AuthenticatorCodeInput'

// Misc
import { collectMessages } from '../../../api/kratosFlows'
import { AUTHENTICATOR_NICKNAME_MAX_LENGTH } from '../../../constants'
import { fieldErrorText } from '../../Auth/kratosPresentation'
import { readAuthenticator } from './securityPresentation'
import { storeAuthenticatorNickname } from './twoFactorNickname'

// Authenticator apps show six digits.
const CODE_LENGTH = 6

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
  // The nickname to start from, when moving to a new app.
  initialNickname: string
  // Called once the app is set up, to close the modal.
  onEnrolled: () => void
}

// Setting up an authenticator app, as a modal's body and footer: an optional nickname, the QR
// code and key to scan, and the six-digit code that proves it worked.
export function AuthenticatorEnrollment(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ nickname, setNickname ] = useState(props.initialNickname)
  const [ code, setCode ] = useState('')
  const [ isSaving, setIsSaving ] = useState(false)
  const authenticator = readAuthenticator(props.flow)
  const messages = collectMessages(props.flow)
  const canTurnOn = authenticator.kind === 'available' && code.length === CODE_LENGTH && !isSaving

  async function turnOn(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    const isEnrolled = await props.onSubmit(
      { method: 'totp', totp_code: code },
      t('security.twoFactor.enrolled'),
    )
    if (!isEnrolled) {
      setCode('')
      setIsSaving(false)
      return
    }

    // Kratos holds the app now, so a nickname that fails to save leaves it on and says so.
    try {
      await storeAuthenticatorNickname(nickname.trim())
    }
    catch (error) {
      console.debug('AuthenticatorEnrollment could not save the nickname', { error })
      toast.warning(t('security.twoFactor.nicknameFailed'))
    }
    setIsSaving(false)
    props.onEnrolled()
  }

  return <Form onSubmit={turnOn} validationBehavior='aria'>
    <Modal.Body className='mt-2 flex flex-col gap-5'>
      <TextField
        autoFocus
        maxLength={AUTHENTICATOR_NICKNAME_MAX_LENGTH}
        autoComplete='off'
        value={nickname}
        onChange={setNickname}
      >
        <Label>{t('security.twoFactor.nickname')}</Label>
        <Input />
        <Description>{t('security.twoFactor.nicknameHint')}</Description>
      </TextField>

      {/* Once the app is set up the modal is closing, so only a flow still loading waits. */}
      {authenticator.kind === 'unavailable' && <div className='grid place-items-center py-6'>
        <Spinner />
      </div>}

      {authenticator.kind === 'available' && <div className='flex flex-col gap-5 sm:flex-row sm:items-start'>
        <img
          src={authenticator.qrCode}
          alt={t('security.twoFactor.qrAlt')}
          className='size-40 shrink-0 rounded-lg bg-white p-2'
        />
        <div className='min-w-0'>
          <p className='compact text-sm'>{t('security.twoFactor.scan')}</p>
          <p className='compact text-xs opacity-70'>{t('security.twoFactor.manualKey')}</p>
          <code className='relaxed block break-all rounded-md bg-surface-secondary px-3 py-2 text-sm'>{
            authenticator.secretKey
          }</code>
          <p className='compact text-sm'>{t('security.twoFactor.enterCode')}</p>
          <AuthenticatorCodeInput
            label={t('fields.authenticatorCode')}
            value={code}
            isDisabled={isSaving}
            isInvalid={Boolean(messages.byField.totp_code)}
            onChange={setCode}
          />
          {messages.byField.totp_code && <p className='mt-2 text-xs text-danger'>{
            fieldErrorText(messages.byField.totp_code, t)
          }</p>}
        </div>
      </div>}
    </Modal.Body>
    <Modal.Footer>
      <Button slot='close' variant='tertiary'>
        <span>{t('common:actions.cancel')}</span>
      </Button>
      <Button type='submit' isDisabled={!canTurnOn} isPending={isSaving}>
        <span>{t('security.twoFactor.turnOn')}</span>
      </Button>
    </Modal.Footer>
  </Form>
}
