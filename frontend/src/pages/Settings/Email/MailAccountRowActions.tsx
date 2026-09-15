// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { MailAccount } from '../../../api/routes/mailRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

export type MailAccountActions = {
  onTest: (account: MailAccount) => void
  onSendTest: (account: MailAccount) => void
  onRename: (account: MailAccount) => void
  onToggleActive: (account: MailAccount) => void
  onDisconnect: (account: MailAccount) => void
}

type Props = MailAccountActions & {
  account: MailAccount
}

// The per-row "..." menu.
export function MailAccountRowActions(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])

  const actions: Record<string, () => void> = {
    test: () => props.onTest(props.account),
    sendTest: () => props.onSendTest(props.account),
    rename: () => props.onRename(props.account),
    toggle: () => props.onToggleActive(props.account),
    disconnect: () => props.onDisconnect(props.account),
  }
  const toggleLabel = props.account.isActive
    ? t('actions.deactivate')
    : t('actions.activate')
  const disconnectLabel = props.account.kind === 'self-hosted'
    ? t('actions.deleteMailbox')
    : t('actions.disconnect')

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
        <Dropdown.Item id='test' textValue={t('actions.test')}>
          <Label>{t('actions.test')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='sendTest' textValue={t('actions.sendTest')}>
          <Label>{t('actions.sendTest')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='rename' textValue={t('actions.rename')}>
          <Label>{t('actions.rename')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='toggle' textValue={toggleLabel}>
          <Label>{toggleLabel}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='disconnect' textValue={disconnectLabel} variant='danger'>
          <Label>{disconnectLabel}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
