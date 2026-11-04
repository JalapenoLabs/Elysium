// Copyright © 2026 Jalapeno Labs

import type { UseOverlayStateReturn } from '@heroui/react'
import type { SettingsFlow } from '@ory/client-fetch'
import type { FormEvent } from 'react'
import type { SettingsSubmission } from '../../../hooks/useKratosSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Description, Form, Input, Label, Modal, TextField, toast } from '@heroui/react'
import { AuthenticatorEnrollment } from './AuthenticatorEnrollment'

// Misc
import { AUTHENTICATOR_NICKNAME_MAX_LENGTH } from '../../../constants'
import { storeAuthenticatorNickname } from './twoFactorNickname'

type Props = {
  state: UseOverlayStateReturn
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
  nickname: string
}

type Step = 'rename' | 'switch'

// Renames the authenticator app, or moves two-factor authentication to a new one. Kratos holds
// one app at a time and only offers a new one's QR code once the old one is removed, so moving
// removes the old app first, then sets the new one up in the same modal.
export function ChangeTwoFactorModal(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const [ step, setStep ] = useState<Step>('rename')
  const [ nickname, setNickname ] = useState(props.nickname)
  const [ isSaving, setIsSaving ] = useState(false)
  const [ isSwitching, setIsSwitching ] = useState(false)

  async function rename(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    try {
      await storeAuthenticatorNickname(nickname.trim())
      toast.success(t('security.twoFactor.nicknameSaved'))
      props.state.close()
    }
    catch (error) {
      console.debug('ChangeTwoFactorModal could not save the nickname', { error })
      toast.danger(t('common:errors.unexpected'))
    }
    setIsSaving(false)
  }

  async function startSwitch() {
    setIsSwitching(true)
    const isRemoved = await props.onSubmit(
      { method: 'totp', totp_unlink: true },
      t('security.twoFactor.switchStarted'),
    )
    setIsSwitching(false)
    if (isRemoved) {
      setStep('switch')
    }
  }

  // Controlled overlays skip the Modal root: it is a trigger wrapper, and without a
  // pressable child React Aria warns. The backdrop takes the open state directly.
  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-xl'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('security.twoFactor.changeTitle')}</Modal.Heading>
        </Modal.Header>

        {step === 'switch' && <AuthenticatorEnrollment
          flow={props.flow}
          onSubmit={props.onSubmit}
          initialNickname={nickname.trim()}
          onEnrolled={props.state.close}
        />}

        {step === 'rename' && <Form onSubmit={rename} validationBehavior='aria'>
          <Modal.Body className='mt-2 flex flex-col gap-5'>
            <TextField
              autoFocus
              maxLength={AUTHENTICATOR_NICKNAME_MAX_LENGTH}
              autoComplete='off'
              value={nickname}
              onChange={setNickname}
            >
              <Label>{t('security.twoFactor.nickname')}</Label>
              <Input />
              <Description>{t('security.twoFactor.nicknameHint')}</Description>
            </TextField>

            <div className='rounded-xl border border-separator p-4'>
              <h3 className='text-sm font-semibold'>{t('security.twoFactor.switchHeading')}</h3>
              <p className='compact mt-1 text-sm opacity-70'>{t('security.twoFactor.switchWarning')}</p>
              <Button variant='outline' size='sm' isPending={isSwitching} onPress={startSwitch}>
                <span>{t('security.twoFactor.switch')}</span>
              </Button>
            </div>
          </Modal.Body>
          <Modal.Footer>
            <Button slot='close' variant='tertiary'>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button
              type='submit'
              isDisabled={nickname.trim() === props.nickname || isSwitching}
              isPending={isSaving}
            >
              <span>{t('common:actions.save')}</span>
            </Button>
          </Modal.Footer>
        </Form>}
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
