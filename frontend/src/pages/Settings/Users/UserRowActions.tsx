// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { User } from '../../../api/routes/userRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

type Props = {
  user: User
  onSetDisabled: (user: User, disabled: boolean) => void
  onRevokeSessions: (user: User) => void
  onResetMfa: (user: User) => void
  onCreateRecoveryLink: (user: User) => void
}

// The per-row "..." menu: disable or enable, sign out everywhere, reset the authenticator,
// or make a recovery link.
export function UserRowActions(props: Props) {
  const { t } = useTranslation([ 'users', 'common' ])
  const isDisabled = props.user.status === 'disabled'

  const actions: Record<string, () => void> = {
    toggle: () => props.onSetDisabled(props.user, !isDisabled),
    revoke: () => props.onRevokeSessions(props.user),
    resetMfa: () => props.onResetMfa(props.user),
    recovery: () => props.onCreateRecoveryLink(props.user),
  }
  const toggleLabel = isDisabled
    ? t('actions.enable')
    : t('actions.disable')

  return <Dropdown>
    <Button
      isIconOnly
      size='sm'
      variant='ghost'
      aria-label={t('common:actions.moreActions')}
    >
      <LuEllipsis className='size-4' aria-hidden />
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu onAction={(key: Key) => actions[String(key)]?.()}>
        <Dropdown.Item id='recovery' textValue={t('actions.recoveryLink')}>
          <Label>{t('actions.recoveryLink')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='revoke' textValue={t('actions.revokeSessions')}>
          <Label>{t('actions.revokeSessions')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='resetMfa' textValue={t('actions.resetMfa')}>
          <Label>{t('actions.resetMfa')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='toggle' textValue={toggleLabel} variant={isDisabled
          ? 'default'
          : 'danger'}>
          <Label>{toggleLabel}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
