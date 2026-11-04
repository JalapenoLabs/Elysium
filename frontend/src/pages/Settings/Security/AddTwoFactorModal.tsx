// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Modal } from '@heroui/react'
import { AuthenticatorEnrollment } from './AuthenticatorEnrollment'

type Props = {
  state: UseOverlayStateReturn
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// Turns two-factor authentication on with an authenticator app.
export function AddTwoFactorModal(props: Props) {
  const { t } = useTranslation('auth')

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('security.twoFactor.addTitle')}</Modal.Heading>
        </Modal.Header>
        <AuthenticatorEnrollment
          flow={props.flow}
          onSubmit={props.onSubmit}
          initialNickname=''
          onEnrolled={props.state.close}
        />
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
