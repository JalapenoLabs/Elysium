// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Project } from '../../api/routes/projectRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

type Props = {
  project: Project
  onEdit: (project: Project) => void
  onDelete: (project: Project) => void
}

// The per-row "..." menu: edit or delete.
export function ProjectRowActions(props: Props) {
  const { t } = useTranslation('common')

  const actions: Record<string, () => void> = {
    edit: () => props.onEdit(props.project),
    delete: () => props.onDelete(props.project),
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
