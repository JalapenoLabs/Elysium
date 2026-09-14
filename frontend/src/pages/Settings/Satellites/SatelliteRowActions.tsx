// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Satellite } from '../../../api/routes/satelliteRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

type Props = {
  satellite: Satellite
  onEdit: (satellite: Satellite) => void
  onTest: (satellite: Satellite) => void
  onToggleActive: (satellite: Satellite) => void
  onDelete: (satellite: Satellite) => void
}

// The per-row "..." menu: edit, test the connection, flip the active flag, or delete.
export function SatelliteRowActions(props: Props) {
  const { t } = useTranslation([ 'satellites', 'common' ])

  const actions: Record<string, () => void> = {
    edit: () => props.onEdit(props.satellite),
    test: () => props.onTest(props.satellite),
    toggle: () => props.onToggleActive(props.satellite),
    delete: () => props.onDelete(props.satellite),
  }
  const toggleLabel = props.satellite.isActive
    ? t('actions.deactivate')
    : t('actions.activate')

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
        <Dropdown.Item id='toggle' textValue={toggleLabel}>
          <Label>{toggleLabel}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
          <Label>{t('common:actions.delete')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
