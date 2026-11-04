// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, useOverlayState } from '@heroui/react'
import { LuKeyRound } from 'react-icons/lu'
import { ChangePasswordModal } from './ChangePasswordModal'
import { SecurityCard } from './SecurityCard'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// The password every account has, changed in a modal.
export function PasswordCard(props: Props) {
  const { t } = useTranslation('auth')
  const modal = useOverlayState()
  // Remounting the modal per opening starts its fields empty.
  const [ opening, setOpening ] = useState(0)

  function open() {
    setOpening((current) => current + 1)
    modal.open()
  }

  return <>
    <SecurityCard
      title={t('security.password.title')}
      actions={<Button variant='outline' onPress={open}>
        <LuKeyRound className='size-4' aria-hidden />
        <span>{t('security.password.change')}</span>
      </Button>}
    />
    <ChangePasswordModal
      key={opening}
      state={modal}
      flow={props.flow}
      onSubmit={props.onSubmit}
    />
  </>
}
