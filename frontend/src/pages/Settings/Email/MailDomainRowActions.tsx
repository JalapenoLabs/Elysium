// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { MailDomain } from '../../../api/routes/mailRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

export type MailDomainActions = {
  onShowDns: (domain: MailDomain) => void
  onRemove: (domain: MailDomain) => void
}

type Props = MailDomainActions & {
  domain: MailDomain
}

// The per-row "..." menu. The default domain cannot be removed, so its Remove is disabled.
export function MailDomainRowActions(props: Props) {
  const { t } = useTranslation([ 'email', 'common' ])

  const actions: Record<string, () => void> = {
    dns: () => props.onShowDns(props.domain),
    remove: () => props.onRemove(props.domain),
  }

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
      <Dropdown.Menu
        disabledKeys={props.domain.isDefault
          ? [ 'remove' ]
          : []}
        onAction={(key: Key) => actions[String(key)]?.()}
      >
        <Dropdown.Item id='dns' textValue={t('domains.actions.dns')}>
          <Label>{t('domains.actions.dns')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='remove' textValue={t('domains.actions.remove')} variant='danger'>
          <Label>{t('domains.actions.remove')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
