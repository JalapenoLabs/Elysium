// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { StorageLocation } from '../../../api/routes/storageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

type Props = {
  location: StorageLocation
  onEdit: (location: StorageLocation) => void
  onTest: (location: StorageLocation) => void
  onDelete: (location: StorageLocation) => void
}

// The per-row "..." menu: edit, test the connection, or delete.
export function StorageLocationRowActions(props: Props) {
  const { t } = useTranslation([ 'storage', 'common' ])

  const actions: Record<string, () => void> = {
    edit: () => props.onEdit(props.location),
    test: () => props.onTest(props.location),
    delete: () => props.onDelete(props.location),
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
      <Dropdown.Menu onAction={(key: Key) => actions[String(key)]?.()}>
        <Dropdown.Item id='edit' textValue={t('common:actions.edit')}>
          <Label>{t('common:actions.edit')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='test' textValue={t('actions.test')}>
          <Label>{t('actions.test')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
          <Label>{t('common:actions.delete')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
