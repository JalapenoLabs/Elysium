// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from './useSettingsFlow'
import type { FormEvent } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, FieldError, Form, Input, Label, TextField } from '@heroui/react'
import { SecuritySection } from './SecuritySection'

// Misc
import { collectMessages } from '../../../api/kratosFlows'
import { fieldErrorText } from '../../Auth/kratosPresentation'
import { readProfile } from './securityPresentation'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// The name everyone in the workspace sees, and the email this person signs in with.
export function ProfileSection(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const stored = readProfile(props.flow)
  const [ name, setName ] = useState(stored.name)
  const [ email, setEmail ] = useState(stored.email)
  const [ isSaving, setIsSaving ] = useState(false)
  const messages = collectMessages(props.flow)
  const isChanged = name.trim() !== stored.name || email.trim() !== stored.email

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    await props.onSubmit(
      { method: 'profile', traits: { name: name.trim(), email: email.trim() }},
      t('security.profile.saved'),
    )
    setIsSaving(false)
  }

  return <SecuritySection
    title={t('security.profile.title')}
    description={t('security.profile.description')}
  >
    <Form onSubmit={save} validationBehavior='aria'>
      <div className='compact grid gap-4 md:grid-cols-2'>
        <TextField
          isRequired
          maxLength={100}
          autoComplete='name'
          isInvalid={Boolean(messages.byField['traits.name'])}
          value={name}
          onChange={setName}
        >
          <Label>{t('fields.name')}</Label>
          <Input />
          <FieldError>{fieldErrorText(messages.byField['traits.name'], t)}</FieldError>
        </TextField>
        <TextField
          isRequired
          type='email'
          autoComplete='email'
          isInvalid={Boolean(messages.byField['traits.email'])}
          value={email}
          onChange={setEmail}
        >
          <Label>{t('fields.email')}</Label>
          <Input />
          <FieldError>{fieldErrorText(messages.byField['traits.email'], t)}</FieldError>
        </TextField>
      </div>
      <Button
        type='submit'
        size='sm'
        isDisabled={!isChanged || !name.trim() || !email.trim() || isSaving}
        isPending={isSaving}
      >
        <span>{t('common:actions.save')}</span>
      </Button>
    </Form>
  </SecuritySection>
}
