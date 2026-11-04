// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { SettingsFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, FieldError, Form, Input, Label, Modal, TextField } from '@heroui/react'
import { PasswordInput } from '../../../components/PasswordInput'

// Misc
import { collectMessages } from '../../../api/kratosFlows'
import { usePasswordAssessment } from '../../../hooks/usePasswordAssessment'
import { fieldErrorText } from '../../Auth/kratosPresentation'
import { readProfile } from '../Profile/profilePresentation'

type Props = {
  state: UseOverlayStateReturn
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// A new password, typed twice. Saving it signs this person out everywhere else. After a
// recovery link, this is where they set the password they forgot.
export function ChangePasswordModal(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const profile = readProfile(props.flow)
  const [ password, setPassword ] = useState('')
  const [ confirmation, setConfirmation ] = useState('')
  const [ isSaving, setIsSaving ] = useState(false)
  const assessment = usePasswordAssessment(password, [ profile.firstName, profile.lastName, profile.email ])
  const messages = collectMessages(props.flow)
  const isMismatched = confirmation.length > 0 && confirmation !== password
  const canSave = assessment.isAcceptable && confirmation === password && !isSaving

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    const isSaved = await props.onSubmit({ method: 'password', password }, t('security.password.saved'))
    setIsSaving(false)
    if (isSaved) {
      props.state.close()
    }
  }

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('security.password.modalTitle')}</Modal.Heading>
          <p className='mt-1 text-sm opacity-70'>{t('security.password.signsOutOthers')}</p>
        </Modal.Header>
        <Form onSubmit={save} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-4'>
            <PasswordInput
              autoFocus
              label={t('fields.newPassword')}
              value={password}
              onChange={setPassword}
              assessment={assessment}
              errorMessage={fieldErrorText(messages.byField.password, t)}
            />
            <TextField
              isRequired
              type='password'
              autoComplete='new-password'
              isInvalid={isMismatched}
              value={confirmation}
              onChange={setConfirmation}
            >
              <Label>{t('fields.confirmPassword')}</Label>
              <Input />
              <FieldError>{t('security.password.mismatch')}</FieldError>
            </TextField>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button type='submit' isDisabled={!canSave} isPending={isSaving}>
              <span>{t('security.password.change')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
