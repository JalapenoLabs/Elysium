// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, buttonVariants, Form, Input, Label, Link, TextField } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'

// Misc
import { useInitialFocus } from '../../../hooks/useInitialFocus'
import { AUTHENTICATOR_NICKNAME_MAX_LENGTH } from '../../../constants'
import { UrlTree } from '../../../urls'

type Props = {
  // The name already given, when coming back to this step.
  initialName: string
  onNext: (name: string) => void
  // Follows what is typed, so leaving with a name half typed asks first.
  onDraftChange: (name: string) => void
}

// The first step of adding two-factor authentication: what the person calls the app, so the
// backup codes file and Security both say which app holds Elysium's codes.
export function TwoFactorNameStep(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ name, setName ] = useState(props.initialName)
  const nameRef = useInitialFocus()
  const trimmed = name.trim()

  function next(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    if (!trimmed) {
      console.debug('TwoFactorNameStep kept the step: the app has no name')
      return
    }
    props.onNext(trimmed)
  }

  return <Form onSubmit={next} validationBehavior='aria'>
    <TextField
      id='two-factor-nickname'
      className='relaxed max-w-md'
      isRequired
      maxLength={AUTHENTICATOR_NICKNAME_MAX_LENGTH}
      value={name}
      onChange={(value) => {
        setName(value)
        props.onDraftChange(value)
      }}
    >
      <Label>{t('security.twoFactor.nickname')}</Label>
      <Input ref={nameRef} autoComplete='off' placeholder={t('security.twoFactor.nicknamePlaceholder')} />
    </TextField>

    <div className='page-create-footer w-full'>
      <Link href={UrlTree.settingsSecurity} className={buttonVariants({ variant: 'outline' })}>
        <LuArrowLeft className='size-4' aria-hidden />
        <span>{t('common:actions.back')}</span>
      </Link>
      <Button type='submit' isDisabled={!trimmed}>
        <span>{t('common:actions.next')}</span>
      </Button>
    </div>
  </Form>
}
