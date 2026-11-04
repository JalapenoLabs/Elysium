// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
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
import { usePageSave } from '../../../layout/page/pageSave'
import { readProfile } from './profilePresentation'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// The name everyone in the workspace sees, and the email this person signs in with, saved to
// Kratos from the page's Save.
export function AccountForm(props: Props) {
  const { t } = useTranslation([ 'auth', 'settings' ])
  const stored = readProfile(props.flow)
  const [ firstName, setFirstName ] = useState(stored.firstName)
  const [ lastName, setLastName ] = useState(stored.lastName)
  const [ email, setEmail ] = useState(stored.email)
  const messages = collectMessages(props.flow)

  const isDirty = firstName.trim() !== stored.firstName
    || lastName.trim() !== stored.lastName
    || email.trim() !== stored.email
  let blockedReason: string | null = null
  if (!firstName.trim()) {
    blockedReason = t('settings:account.firstNameRequired')
  }
  else if (!email.trim()) {
    blockedReason = t('settings:account.emailRequired')
  }

  const onSubmit = usePageSave({
    save: () => props.onSubmit(
      {
        method: 'profile',
        traits: {
          email: email.trim(),
          name: { first: firstName.trim(), last: lastName.trim() },
        },
      },
      t('settings:account.saved'),
    ),
    isDirty,
    blockedReason,
  })

  return <Form onSubmit={onSubmit} validationBehavior='aria' className='max-w-2xl'>
    <div className='compact'>
      <PersonNameFields
        shouldFocus
        idPrefix='account'
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
      id='account-email'
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
