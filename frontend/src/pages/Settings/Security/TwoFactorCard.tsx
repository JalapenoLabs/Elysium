// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { selectMe } from '../../../store/authSlice'
import { useAppSelector } from '../../../store/hooks'

// User interface
import { Button, Chip, useOverlayState } from '@heroui/react'
import { LuPencil, LuShieldPlus, LuTrash2 } from 'react-icons/lu'
import { AddTwoFactorModal } from './AddTwoFactorModal'
import { ChangeTwoFactorModal } from './ChangeTwoFactorModal'
import { SecurityCard } from './SecurityCard'

// Misc
import { useConfirm } from '../../../hooks/useConfirm'
import { readAuthenticator, readLookupCodes } from './securityPresentation'
import { storeAuthenticatorNickname } from './twoFactorNickname'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

// Whether two-factor authentication is on and what it uses. Setting it up, changing it, and
// removing it each happen in a modal, so the QR code and key show only while setting one up.
export function TwoFactorCard(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const me = useAppSelector(selectMe)
  const addModal = useOverlayState()
  const changeModal = useOverlayState()
  // Remounting a modal per opening starts it from what is stored now.
  const [ opening, setOpening ] = useState(0)
  const authenticator = readAuthenticator(props.flow)
  const lookup = readLookupCodes(props.flow)
  const isOn = authenticator.kind === 'enrolled'
  const hasLookupCodes = lookup.canReveal || lookup.canDisable
  const nickname = me?.authenticatorNickname ?? ''

  function open(modal: typeof addModal) {
    setOpening((current) => current + 1)
    modal.open()
  }

  function remove() {
    confirm({
      title: t('security.twoFactor.removeTitle'),
      message: t('security.twoFactor.removeMessage', {
        name: nickname || t('security.twoFactor.unnamedApp'),
      }),
      isIrreversible: true,
      confirmText: t('security.twoFactor.remove'),
      onConfirm: async () => {
        const isRemoved = await props.onSubmit(
          { method: 'totp', totp_unlink: true },
          t('security.twoFactor.removed'),
        )
        if (!isRemoved) {
          return
        }
        // The app is gone either way; its name only shows beside an app, so a failure here
        // is harmless and the next app set up replaces it.
        await storeAuthenticatorNickname('').catch((error: unknown) => {
          console.debug('TwoFactorCard could not forget the nickname', { error })
        })
      },
    })
  }

  const authenticatorState = isOn
    ? nickname || t('security.twoFactor.on')
    : t('security.twoFactor.notSetUp')

  return <>
    <SecurityCard
      title={t('security.twoFactor.title')}
      status={<Chip size='sm' variant='soft' color={isOn
        ? 'success'
        : 'default'}>{
          isOn
            ? t('security.twoFactor.on')
            : t('security.twoFactor.off')
        }</Chip>}
      actions={isOn
        ? <>
          <Button variant='outline' onPress={() => open(changeModal)}>
            <LuPencil className='size-4' aria-hidden />
            <span>{t('security.twoFactor.change')}</span>
          </Button>
          <Button variant='ghost' onPress={remove}>
            <LuTrash2 className='size-4' aria-hidden />
            <span>{t('security.twoFactor.remove')}</span>
          </Button>
        </>
        : <Button
          variant='outline'
          isDisabled={authenticator.kind !== 'available'}
          onPress={() => open(addModal)}
        >
          <LuShieldPlus className='size-4' aria-hidden />
          <span>{t('security.twoFactor.add')}</span>
        </Button>}
    >
      <dl className='grid max-w-md grid-cols-[auto_1fr] gap-x-6 gap-y-1 text-sm'>
        <dt className='opacity-70'>{t('security.twoFactor.authenticatorApp')}</dt>
        <dd>{authenticatorState}</dd>
        <dt className='opacity-70'>{t('security.twoFactor.lookupCodes')}</dt>
        <dd>{
          hasLookupCodes
            ? t('security.twoFactor.on')
            : t('security.twoFactor.off')
        }</dd>
      </dl>
    </SecurityCard>

    <AddTwoFactorModal
      key={`add-${opening}`}
      state={addModal}
      flow={props.flow}
      onSubmit={props.onSubmit}
    />
    <ChangeTwoFactorModal
      key={`change-${opening}`}
      state={changeModal}
      flow={props.flow}
      onSubmit={props.onSubmit}
      nickname={nickname}
    />
  </>
}
