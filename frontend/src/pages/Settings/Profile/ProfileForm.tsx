// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { FieldError, Form, Input, Label, TextField } from '@heroui/react'
import { PersonNameFields } from '../../../components/PersonNameFields'

// Misc
import { collectMessages } from '../../../api/kratosFlows'
import { fieldErrorText } from '../../Auth/kratosPresentation'
import { useSettingsSave } from '../settingsSave'
import { readProfile } from './profilePresentation'

const PROFILE_FORM_ID = 'profile-form'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// The name everyone in the workspace sees, and the email this person signs in with, saved to
// Kratos from the page's Save.
export function ProfileForm(props: Props) {
  const { t } = useTranslation([ 'auth', 'settings' ])
  const stored = readProfile(props.flow)
  const [ firstName, setFirstName ] = useState(stored.firstName)
  const [ lastName, setLastName ] = useState(stored.lastName)
  const [ email, setEmail ] = useState(stored.email)
  const [ isSaving, setIsSaving ] = useState(false)
  const messages = collectMessages(props.flow)

  const isDirty = firstName.trim() !== stored.firstName
    || lastName.trim() !== stored.lastName
    || email.trim() !== stored.email
  let blockedReason: string | null = null
  if (!firstName.trim()) {
    blockedReason = t('settings:profile.firstNameRequired')
  }
  else if (!email.trim()) {
    blockedReason = t('settings:profile.emailRequired')
  }

  useSettingsSave({
    formId: PROFILE_FORM_ID,
    isDirty,
    isPending: isSaving,
    blockedReason,
  })

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    await props.onSubmit(
      {
        method: 'profile',
        traits: {
          email: email.trim(),
          name: { first: firstName.trim(), last: lastName.trim() },
        },
      },
      t('settings:profile.saved'),
    )
    setIsSaving(false)
  }

  return <Form id={PROFILE_FORM_ID} onSubmit={save} validationBehavior='aria' className='max-w-2xl'>
    <div className='compact'>
      <PersonNameFields
        autoFocus
        firstName={firstName}
        lastName={lastName}
        onFirstNameChange={setFirstName}
        onLastNameChange={setLastName}
        firstNameError={fieldErrorText(messages.byField['traits.name.first'], t)}
        lastNameError={fieldErrorText(messages.byField['traits.name.last'], t)}
      />
    </div>
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
  </Form>
}
