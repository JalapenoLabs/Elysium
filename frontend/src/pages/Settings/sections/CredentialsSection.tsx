// Copyright © 2026 Jalapeno Labs

import type { TotpMethodSummary } from '../../../api/routes/userRoutes'

// Core
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

// UI
import {
  Button,
  Chip,
  Dropdown,
  DropdownItem,
  DropdownMenu,
  DropdownTrigger,
} from '@heroui/react'
import { LuEllipsisVertical, LuPlus } from 'react-icons/lu'
import { SettingsSection } from '../../../components/settings/SettingsSection'
import { AddAuthenticatorModal } from '../AddAuthenticatorModal'

// Utility
import moment from 'moment-timezone'
import { HTTPError } from 'ky'

// Misc
import { useConfirm } from '../../../hooks/useConfirm'
import { usePrompt } from '../../../hooks/usePrompt'
import { enrollPasskey } from '../../../api/routes/authRoutes'
import {
  deletePasskey,
  deleteTotpMethod,
  getCredentials,
  renamePasskey,
  renameTotpMethod,
  setDefaultTotpMethod,
  type CredentialsSummary,
} from '../../../api/routes/userRoutes'

// The credential sections: passkeys and two-step authenticators (each a named,
// individually managed list) plus the password status. Naming lets a user tell
// several credentials apart; each can be renamed or removed, and one authenticator
// is the default. Flat rows, no cards, matching the rest of the page.
export function CredentialsSection() {
  const { t } = useTranslation([ 'auth', 'common' ])
  const confirm = useConfirm()
  const prompt = usePrompt()

  const [ credentials, setCredentials ] = useState<CredentialsSummary | null>(null)
  const [ isEnrolling, setIsEnrolling ] = useState(false)
  const [ actionError, setActionError ] = useState<string | null>(null)
  const [ isAuthenticatorOpen, setIsAuthenticatorOpen ] = useState(false)

  async function loadCredentials() {
    try {
      setCredentials(await getCredentials())
    }
    catch (error) {
      console.debug('credentials unavailable', { error })
    }
  }

  useEffect(() => {
    loadCredentials()
  }, [])

  async function onAddPasskey() {
    setIsEnrolling(true)
    setActionError(null)
    try {
      await enrollPasskey()
      await loadCredentials()
    }
    catch (error) {
      console.debug('passkey enrollment failed', { error })
      setActionError(t('auth:settings.security.passkeyFailed'))
    }
    finally {
      setIsEnrolling(false)
    }
  }

  // Prompts for a new label, then renames the credential and reloads the list.
  function onRename(currentName: string, rename: (name: string) => Promise<Response>) {
    prompt({
      title: t('auth:settings.security.renameTitle'),
      defaultValue: currentName,
      inputProps: { label: t('auth:settings.security.renameLabel') },
      confirmLabel: t('common:actions.save'),
      validate: (value) => (value.trim() ? null : t('auth:settings.security.renameRequired')),
      onConfirm: async (value) => {
        await rename(value)
        await loadCredentials()
      },
    })
  }

  // Confirms, then removes the credential. A 409 on the last passkey surfaces the
  // "only way to sign in" explanation instead of failing silently.
  function onRemove(
    title: string,
    message: string,
    remove: () => Promise<Response>,
  ) {
    setActionError(null)
    confirm({
      title,
      message,
      confirmLabel: t('auth:settings.security.removeAction'),
      cancelLabel: t('common:actions.cancel'),
      confirmColor: 'danger',
      onConfirm: async () => {
        try {
          await remove()
        }
        catch (error) {
          if (error instanceof HTTPError && error.response.status === 409) {
            setActionError(t('auth:settings.security.passkeyOnlyCredential'))
            return
          }
          throw error
        }
        await loadCredentials()
      },
    })
  }

  async function onSetDefault(method: TotpMethodSummary) {
    setActionError(null)
    try {
      await setDefaultTotpMethod(method.id)
      await loadCredentials()
    }
    catch (error) {
      console.debug('set default authenticator failed', { error })
      setActionError(t('auth:settings.security.updateFailed'))
    }
  }

  const passkeys = credentials?.passkeys ?? []
  const totpMethods = credentials?.totp_methods ?? []
  const passwordEnrolled = credentials?.password?.enrolled ?? false

  return <>
    <AddAuthenticatorModal
      isOpen={isAuthenticatorOpen}
      onOpenChange={setIsAuthenticatorOpen}
      onEnrolled={loadCredentials}
    />

    <SettingsSection
      title={t('auth:settings.security.passkeysTitle')}
      description={t('auth:settings.security.passkeysBody')}
      action={
        <Button
          size='sm'
          radius='sm'
          variant='bordered'
          startContent={<LuPlus className='h-3.5 w-3.5' />}
          isLoading={isEnrolling}
          onPress={onAddPasskey}
        >
          <span>{ t('auth:settings.security.addPasskey') }</span>
        </Button>
      }
    >
      { actionError
        ? <p className='mb-3 text-sm text-danger'>{ actionError }</p>
        : null }
      { passkeys.length
        ? <div className='divide-y divide-divider'>
          { passkeys.map((passkey) =>
            <CredentialRow
              key={passkey.id}
              name={passkey.name || t('auth:settings.security.passkeyUnnamed')}
              isUnnamed={!passkey.name}
              detail={passkey.last_used_at
                ? t('auth:settings.security.lastUsed', {
                  when: moment(passkey.last_used_at).fromNow(),
                })
                : t('auth:settings.security.neverUsed')}
              onRename={() => onRename(passkey.name, (name) => renamePasskey(passkey.id, name))}
              onRemove={() => onRemove(
                t('auth:settings.security.removePasskeyTitle'),
                t('auth:settings.security.removePasskeyBody', {
                  name: passkey.name || t('auth:settings.security.passkeyUnnamed'),
                }),
                () => deletePasskey(passkey.id),
              )}
            />,
          ) }
        </div>
        : credentials
          ? <p className='text-sm text-default-500'>{ t('auth:settings.security.passkeysEmpty') }</p>
          : null }
    </SettingsSection>

    <SettingsSection
      title={t('auth:settings.security.authenticatorsTitle')}
      description={t('auth:settings.security.authenticatorsBody')}
      action={
        <Button
          size='sm'
          radius='sm'
          variant='bordered'
          startContent={<LuPlus className='h-3.5 w-3.5' />}
          onPress={() => setIsAuthenticatorOpen(true)}
        >
          <span>{ t('auth:settings.security.addAuthenticator') }</span>
        </Button>
      }
    >
      { totpMethods.length
        ? <div className='divide-y divide-divider'>
          { totpMethods.map((method) =>
            <CredentialRow
              key={method.id}
              name={method.name || t('auth:settings.security.authenticatorUnnamed')}
              badge={method.is_default ? t('auth:settings.security.defaultBadge') : undefined}
              detail={method.last_used_at
                ? t('auth:settings.security.lastUsed', {
                  when: moment(method.last_used_at).fromNow(),
                })
                : t('auth:settings.security.neverUsed')}
              onSetDefault={method.is_default ? undefined : () => onSetDefault(method)}
              onRename={() => onRename(method.name, (name) => renameTotpMethod(method.id, name))}
              onRemove={() => onRemove(
                t('auth:settings.security.removeAuthenticatorTitle'),
                t('auth:settings.security.removeAuthenticatorBody', {
                  name: method.name || t('auth:settings.security.authenticatorUnnamed'),
                }),
                () => deleteTotpMethod(method.id),
              )}
            />,
          ) }
        </div>
        : credentials
          ? <Chip size='sm' variant='flat' radius='sm'>
            { t('auth:settings.security.notEnrolled') }
          </Chip>
          : null }
    </SettingsSection>

    <SettingsSection
      title={t('auth:settings.security.passwordTitle')}
      description={t('auth:settings.security.passwordBody')}
    >
      <Chip
        size='sm'
        variant='flat'
        radius='sm'
        color={passwordEnrolled ? 'success' : 'default'}
      >
        { passwordEnrolled ? t('auth:settings.security.enrolled') : t('auth:settings.security.notSet') }
      </Chip>
    </SettingsSection>
  </>
}

type CredentialRowProps = {
  name: string
  // Renders the name muted when it is a placeholder (an unnamed credential).
  isUnnamed?: boolean
  // A small status pill next to the name, e.g. the default authenticator.
  badge?: string
  detail: string
  // Omitted when the credential is already the default.
  onSetDefault?: () => void
  onRename: () => void
  onRemove: () => void
}

// One credential row: name (with an optional badge), a right-aligned detail, and
// a manage menu. Local to this section; not exported.
function CredentialRow(props: CredentialRowProps) {
  const { t } = useTranslation('auth')

  return <div className='flex items-center justify-between py-3 text-sm'>
    <div className='flex items-center gap-2'>
      <span className={props.isUnnamed ? 'italic text-default-400' : undefined}>{ props.name }</span>
      { props.badge
        ? <Chip size='sm' variant='flat' radius='sm' color='success'>{ props.badge }</Chip>
        : null }
    </div>
    <div className='flex items-center gap-3'>
      <span className='text-default-500'>{ props.detail }</span>
      <Dropdown placement='bottom-end'>
        <DropdownTrigger>
          <Button isIconOnly size='sm' variant='light' aria-label={t('settings.security.manage')}>
            <LuEllipsisVertical className='h-4 w-4' />
          </Button>
        </DropdownTrigger>
        <DropdownMenu aria-label={t('settings.security.manage')}>
          { props.onSetDefault
            ? <DropdownItem key='default' onPress={props.onSetDefault}>
              { t('settings.security.setDefault') }
            </DropdownItem>
            : null }
          <DropdownItem key='rename' onPress={props.onRename}>
            { t('settings.security.rename') }
          </DropdownItem>
          <DropdownItem key='remove' color='danger' className='text-danger' onPress={props.onRemove}>
            { t('settings.security.removeAction') }
          </DropdownItem>
        </DropdownMenu>
      </Dropdown>
    </div>
  </div>
}
