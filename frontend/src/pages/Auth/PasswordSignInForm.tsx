// Copyright © 2026 Jalapeno Labs

import type { LoginFlow } from '@ory/client-fetch'
import type { LoginSubmission } from './loginPresentation'
import type { FormEvent } from 'react'

// Core
import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, FieldError, Form, Input, Label, Separator, TextField } from '@heroui/react'
import { LuFingerprint } from 'react-icons/lu'

// Utility
import { browserSupportsWebAuthnAutofill, startAuthentication, WebAuthnAbortService } from '@simplewebauthn/browser'

// Misc
import { collectMessages, hasGroup, inputValue } from '../../api/kratosFlows'
import { fieldErrorText } from './kratosPresentation'
import { readPasskeyRequestOptions } from './passkeyOptions'

type Props = {
  flow: LoginFlow
  // Confirming a recent sign-in: the email is the signed-in person's, and fixed.
  isRefresh: boolean
  onSubmit: (body: LoginSubmission) => Promise<void>
}

// Email and password, or a passkey. Passkeys also appear among the browser's suggestions
// for the email field (conditional UI), where the browser supports that.
export function PasswordSignInForm(props: Props) {
  const { t } = useTranslation('auth')
  const [ email, setEmail ] = useState(() => inputValue(props.flow, 'identifier'))
  const [ password, setPassword ] = useState('')
  const [ isSubmitting, setIsSubmitting ] = useState(false)
  const [ passkeyError, setPasskeyError ] = useState<string | null>(null)
  const emailRef = useRef<HTMLInputElement>(null)
  const hasPasskey = hasGroup(props.flow, 'passkey')
  const challenge = inputValue(props.flow, 'passkey_challenge')
  const messages = collectMessages(props.flow)
  const { onSubmit } = props

  useEffect(() => {
    if (props.isRefresh) {
      return
    }
    emailRef.current?.focus()
  }, [ props.isRefresh ])

  // Offers passkeys in the email field's autofill while the form is open. Picking one
  // signs in at once; anything else (dismissing it, typing) simply leaves the form as is.
  useEffect(() => {
    let isCurrent = true
    if (challenge && !props.isRefresh) {
      browserSupportsWebAuthnAutofill()
        .then(async (isSupported) => {
          if (!isSupported || !isCurrent) {
            return
          }
          const credential = await startAuthentication({
            optionsJSON: readPasskeyRequestOptions(challenge),
            useBrowserAutofill: true,
          })
          await onSubmit({ method: 'passkey', passkey_login: JSON.stringify(credential) })
        })
        .catch((error: unknown) => {
          console.debug('PasswordSignInForm passkey autofill ended without signing in', { error })
        })
    }
    return () => {
      isCurrent = false
      WebAuthnAbortService.cancelCeremony()
    }
  }, [ challenge, props.isRefresh, onSubmit ])

  async function signInWithPassword(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSubmitting(true)
    await onSubmit({ method: 'password', identifier: email.trim(), password })
    setIsSubmitting(false)
  }

  async function signInWithPasskey() {
    setPasskeyError(null)
    setIsSubmitting(true)
    try {
      const credential = await startAuthentication({ optionsJSON: readPasskeyRequestOptions(challenge) })
      await onSubmit({ method: 'passkey', passkey_login: JSON.stringify(credential) })
    }
    catch (error) {
      // Most often the person closed the passkey prompt.
      console.debug('PasswordSignInForm passkey sign-in did not finish', { error })
      setPasskeyError(t('login.passkeyFailed'))
    }
    setIsSubmitting(false)
  }

  const canSubmit = email.trim().length > 0 && password.length > 0 && !isSubmitting

  return <div>
    <Form onSubmit={signInWithPassword} validationBehavior='aria'>
      <TextField
        isRequired
        className='compact'
        type='email'
        name='identifier'
        autoComplete='username webauthn'
        isReadOnly={props.isRefresh}
        isInvalid={Boolean(messages.byField.identifier)}
        value={email}
        onChange={setEmail}
      >
        <Label>{t('fields.email')}</Label>
        <Input ref={emailRef} />
        <FieldError>{fieldErrorText(messages.byField.identifier, t)}</FieldError>
      </TextField>
      <TextField
        isRequired
        autoFocus={props.isRefresh}
        className='relaxed'
        type='password'
        name='password'
        autoComplete='current-password'
        isInvalid={Boolean(messages.byField.password)}
        value={password}
        onChange={setPassword}
      >
        <Label>{t('fields.password')}</Label>
        <Input />
        <FieldError>{fieldErrorText(messages.byField.password, t)}</FieldError>
      </TextField>
      <Button
        fullWidth
        type='submit'
        isDisabled={!canSubmit}
        isPending={isSubmitting}
      >
        <span>{
          props.isRefresh
            ? t('login.confirm')
            : t('login.submit')
        }</span>
      </Button>
    </Form>

    {hasPasskey && challenge && <>
      <div className='level my-5 text-xs opacity-60'>
        <Separator className='flex-1' />
        <span>{t('login.or')}</span>
        <Separator className='flex-1' />
      </div>
      <Button
        fullWidth
        variant='outline'
        isDisabled={isSubmitting}
        onPress={signInWithPasskey}
      >
        <LuFingerprint className='size-4' aria-hidden />
        <span>{t('login.passkey')}</span>
      </Button>
      {passkeyError && <p className='mt-2 text-center text-xs text-danger'>{passkeyError}</p>}
    </>}
  </div>
}
