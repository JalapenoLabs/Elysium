// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { EnvironmentVariable } from '../../../api/routes/environmentRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

type Props = {
  variable: EnvironmentVariable
  onEdit: (variable: EnvironmentVariable) => void
  onDelete: (variable: EnvironmentVariable) => void
}

// The per-row "..." menu: edit or delete.
export function EnvironmentVariableRowActions(props: Props) {
  const { t } = useTranslation('common')

  const actions: Record<string, () => void> = {
    edit: () => props.onEdit(props.variable),
    delete: () => props.onDelete(props.variable),
  }

  return <Dropdown>
    <Button
      isIconOnly
      size='sm'
      variant='ghost'
      aria-label={t('actions.moreActions')}
    >
      <LuEllipsis className='size-4' aria-hidden />
    </Button>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu onAction={(key: Key) => actions[String(key)]?.()}>
        <Dropdown.Item id='edit' textValue={t('actions.edit')}>
          <Label>{t('actions.edit')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='delete' textValue={t('actions.delete')} variant='danger'>
          <Label>{t('actions.delete')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
