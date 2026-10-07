// Copyright © 2026 Jalapeno Labs

import type { SettingsFlow } from '@ory/client-fetch'
import type { SettingsSubmission } from './useSettingsFlow'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuFingerprint, LuPlus, LuTrash2 } from 'react-icons/lu'
import { SecuritySection } from './SecuritySection'

// Utility
import { startRegistration } from '@simplewebauthn/browser'

// Misc
import { inputValue } from '../../../api/kratosFlows'
import { useConfirm } from '../../../hooks/useConfirm'
import { readPasskeyCreationOptions } from '../../Auth/passkeyOptions'
import { readPasskeys } from './securityPresentation'

type Props = {
  flow: SettingsFlow
  onSubmit: (body: SettingsSubmission, savedMessage: string) => Promise<boolean>
}

const dateFormatter = new Intl.DateTimeFormat(undefined, { dateStyle: 'medium' })

// Passkeys held by a password manager (Bitwarden, 1Password) or the browser, for signing in
// without a password.
export function PasskeysSection(props: Props) {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const [ isAdding, setIsAdding ] = useState(false)
  const passkeys = readPasskeys(props.flow)
  const creationData = inputValue(props.flow, 'passkey_create_data')

  async function add() {
    setIsAdding(true)
    try {
      const credential = await startRegistration({ optionsJSON: readPasskeyCreationOptions(creationData) })
      await props.onSubmit(
        { method: 'passkey', passkey_settings_register: JSON.stringify(credential) },
        t('security.passkeys.added'),
      )
    }
    catch (error) {
      // Most often the person closed the prompt, or the password manager already has one.
      console.debug('PasskeysSection did not add a passkey', { error })
    }
    setIsAdding(false)
  }

  function remove(passkeyId: string, name: string) {
    confirm({
      title: t('security.passkeys.removeTitle'),
      message: t('security.passkeys.removeMessage', { name }),
      tone: 'danger',
      confirmText: t('security.passkeys.remove'),
      onConfirm: async () => {
        await props.onSubmit({ method: 'passkey', passkey_remove: passkeyId }, t('security.passkeys.removed'))
      },
    })
  }

  return <SecuritySection
    title={t('security.passkeys.title')}
    description={t('security.passkeys.description')}
    status={<Button
      size='sm'
      variant='outline'
      isDisabled={!creationData}
      isPending={isAdding}
      onPress={add}
    >
      <LuPlus className='size-4' aria-hidden />
      <span>{t('security.passkeys.add')}</span>
    </Button>}
  >
    {!passkeys.length && <p className='text-sm opacity-70'>{t('security.passkeys.empty')}</p>}
    <ul className='flex flex-col gap-2'>{
      passkeys.map((passkey) => {
        const name = passkey.name || t('security.passkeys.unnamed')
        return <li
        key={passkey.id}
        className='level rounded-lg bg-surface-secondary px-4 py-3'
      >
        <div className='level-left'>
          <LuFingerprint className='size-5 text-accent' aria-hidden />
          <div>
            <p className='text-sm font-medium'>{name}</p>
            {passkey.addedAt && <p className='text-xs opacity-60'>{
              t('security.passkeys.addedOn', { date: dateFormatter.format(new Date(passkey.addedAt)) })
            }</p>}
          </div>
        </div>
        <Tooltip delay={200}>
          <Tooltip.Trigger>
            <div>
              <Button
                isIconOnly
                size='sm'
                variant='ghost'
                isDisabled={!passkey.canRemove}
                aria-label={t('security.passkeys.remove')}
                onPress={() => remove(passkey.id, name)}
              >
                <LuTrash2 className='size-4' aria-hidden />
              </Button>
            </div>
          </Tooltip.Trigger>
          <Tooltip.Content>
            <span>{
              passkey.canRemove
                ? t('security.passkeys.remove')
                : t('security.passkeys.lastWay')
            }</span>
          </Tooltip.Content>
        </Tooltip>
      </li>
      })
    }</ul>
  </SecuritySection>
}
