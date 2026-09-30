// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from './useSettingsFlow'
import type { FormEvent } from 'react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Form } from '@heroui/react'
import { PasswordInput } from '../../../components/PasswordInput'
import { SecuritySection } from './SecuritySection'

// Misc
import { collectMessages } from '../../../api/kratosFlows'
import { usePasswordAssessment } from '../../../hooks/usePasswordAssessment'
import { fieldErrorText } from '../../Auth/kratosPresentation'
import { readProfile } from './securityPresentation'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// A new password, which also signs this person out everywhere else. After a recovery link,
// this is where they set the password they forgot.
export function PasswordSection(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const profile = readProfile(props.flow)
  const [ password, setPassword ] = useState('')
  const [ isSaving, setIsSaving ] = useState(false)
  const assessment = usePasswordAssessment(password, [ profile.name, profile.email ])
  const messages = collectMessages(props.flow)

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    const isSaved = await props.onSubmit({ method: 'password', password }, t('security.password.saved'))
    if (isSaved) {
      setPassword('')
    }
    setIsSaving(false)
  }

  return <SecuritySection
    title={t('security.password.title')}
    description={t('security.password.description')}
  >
    <Form onSubmit={save} validationBehavior='aria' className='max-w-md'>
      <div className='relaxed'>
        <PasswordInput
          label={t('fields.newPassword')}
          value={password}
          onChange={setPassword}
          assessment={assessment}
          errorMessage={fieldErrorText(messages.byField.password, t)}
        />
      </div>
      <Button type='submit' size='sm' isDisabled={!assessment.isAcceptable || isSaving} isPending={isSaving}>
        <span>{t('security.password.submit')}</span>
      </Button>
    </Form>
  </SecuritySection>
}
