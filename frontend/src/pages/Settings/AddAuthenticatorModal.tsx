// Copyright © 2026 Jalapeno Labs

import type { FormEvent } from 'react'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import { Input, Modal, ModalBody, ModalContent, ModalHeader } from '@heroui/react'
import { TotpEnrollCard } from '../../components/enrollment/TotpEnrollCard'

// Misc
import { useTotpEnrollment } from '../../auth/methods/useTotpEnrollment'

type Props = {
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
  // Fired once an authenticator is confirmed, so the Security Center can note it.
  onEnrolled: () => void
}

// Enrolls an additional authenticator app from the Security Center, independent
// of the require-2FA gate. Begins on open (fresh QR each time) and closes on a
// confirmed code.
export function AddAuthenticatorModal(props: Props) {
  const { t } = useTranslation('auth')

  const enrollment = useTotpEnrollment()
  const { begin } = enrollment
  const [ code, setCode ] = useState('')
  const [ name, setName ] = useState('')

  // A fresh provisioning URI each time the modal opens; clear the fields on close.
  useEffect(() => {
    if (props.isOpen) {
      void begin()
    }
    else {
      setCode('')
      setName('')
    }
  }, [ props.isOpen, begin ])

  async function onConfirm(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const confirmed = await enrollment.confirm(code, name)
    if (confirmed) {
      props.onEnrolled()
      props.onOpenChange(false)
    }
  }

  const errorMessage = enrollment.error === 'invalidCode'
    ? t('settings.security.authenticatorInvalid')
    : enrollment.error === 'failed'
      ? t('settings.security.authenticatorFailed')
      : null

  return <Modal isOpen={props.isOpen} onOpenChange={props.onOpenChange}>
    <ModalContent>
      <ModalHeader>{ t('settings.security.addAuthenticator') }</ModalHeader>
      <ModalBody className='pb-6'>
        <TotpEnrollCard
          provisioningUri={enrollment.provisioningUri}
          code={code}
          onCodeChange={setCode}
          onSubmit={onConfirm}
          title={t('settings.security.authenticatorTitle')}
          subtitle={t('settings.security.authenticatorSubtitle')}
          manualHint={t('settings.security.authenticatorManualHint')}
          codeLabel={t('settings.security.authenticatorCodeLabel')}
          actionLabel={t('settings.security.authenticatorAction')}
          requiredReason={t('settings.security.authenticatorRequired')}
          nameField={
            <Input
              size='sm'
              radius='sm'
              label={t('settings.security.authenticatorNameLabel')}
              placeholder={t('settings.security.authenticatorNamePlaceholder')}
              value={name}
              onValueChange={setName}
            />
          }
          errorMessage={errorMessage}
          isBusy={enrollment.isBusy}
        />
      </ModalBody>
    </ModalContent>
  </Modal>
}
