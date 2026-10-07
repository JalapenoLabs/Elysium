// Copyright © 2026 Jalapeno Labs

import type { LoginFlow } from '@ory/client-fetch'
import type { LoginSubmission } from './loginPresentation'
import type { FormEvent } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, FieldError, Form, Input, Label, TextField } from '@heroui/react'
import { AuthenticatorCodeInput } from '../../components/AuthenticatorCodeInput'

// Misc
import { collectMessages, hasGroup } from '../../api/kratosFlows'
import { signOut } from '../../auth/signOut'
import { fieldErrorText } from './kratosPresentation'

type Props = {
  flow: LoginFlow
  onSubmit: (body: LoginSubmission) => Promise<void>
}

type Method = 'totp' | 'lookup_secret'

// The second step for an account with an authenticator app: its six-digit code, or one of
// the lookup codes saved when it was set up, for someone without their phone.
export function SecondFactorForm(props: Props) {
  const { t } = useTranslation('auth')
  const hasTotp = hasGroup(props.flow, 'totp')
  const hasLookup = hasGroup(props.flow, 'lookup_secret')
  const [ method, setMethod ] = useState<Method>(hasTotp
    ? 'totp'
    : 'lookup_secret')
  const [ code, setCode ] = useState('')
  const [ lookupCode, setLookupCode ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const messages = collectMessages(props.flow)

  async function submitCode(totpCode: string) {
    setIsSubmitting(true)
    await props.onSubmit({ method: 'totp', totp_code: totpCode })
    setCode('')
    setIsSubmitting(false)
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (method === 'totp') {
      await submitCode(code)
      return
    }
    setIsSubmitting(true)
    await props.onSubmit({ method: 'lookup_secret', lookup_secret: lookupCode.trim() })
    setIsSubmitting(false)
  }

  const canSubmit = method === 'totp'
    ? code.length === 6
    : lookupCode.trim().length > 0

  return <Form onSubmit={submit} validationBehavior='aria'>
    {method === 'totp'
      ? <div className='relaxed flex flex-col items-center gap-2'>
        <AuthenticatorCodeInput
          autoFocus
          label={t('fields.authenticatorCode')}
          value={code}
          isDisabled={isSubmitting}
          isInvalid={Boolean(messages.byField.totp_code)}
          onChange={setCode}
          onComplete={submitCode}
        />
        {messages.byField.totp_code && <p className='text-xs text-danger'>{
          fieldErrorText(messages.byField.totp_code, t)
        }</p>}
      </div>
      : <TextField
        isRequired
        autoFocus
        className='relaxed'
        autoComplete='off'
        isInvalid={Boolean(messages.byField.lookup_secret)}
        value={lookupCode}
        onChange={setLookupCode}
      >
        <Label>{t('fields.lookupCode')}</Label>
        <Input />
        <FieldError>{fieldErrorText(messages.byField.lookup_secret, t)}</FieldError>
      </TextField>}

    <Button fullWidth type='submit' isDisabled={!canSubmit || isSubmitting} isPending={isSubmitting}>
      <span>{t('login.secondFactor.submit')}</span>
    </Button>

    <div className='mt-5 flex flex-col items-center gap-1 text-sm'>
      {hasTotp && hasLookup && <Button
        size='sm'
        variant='ghost'
        onPress={() => setMethod(method === 'totp'
          ? 'lookup_secret'
          : 'totp')}
      >
        <span>{
          method === 'totp'
            ? t('login.secondFactor.useLookupCode')
            : t('login.secondFactor.useAuthenticator')
        }</span>
      </Button>}
      <Button size='sm' variant='ghost' onPress={() => void signOut()}>
        <span>{t('login.secondFactor.signOut')}</span>
      </Button>
    </div>
  </Form>
}
