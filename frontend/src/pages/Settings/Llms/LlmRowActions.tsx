// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { Llm } from '../../../api/routes/llmRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

type Props = {
  llm: Llm
  onEdit: (llm: Llm) => void
  onToggleActive: (llm: Llm) => void
  onDelete: (llm: Llm) => void
}

// The per-row "..." menu: edit, flip the active flag, or delete.
export function LlmRowActions(props: Props) {
  const { t } = useTranslation([ 'llms', 'common' ])

  const actions: Record<string, () => void> = {
    edit: () => props.onEdit(props.llm),
    toggle: () => props.onToggleActive(props.llm),
    delete: () => props.onDelete(props.llm),
  }
  const toggleLabel = props.llm.isActive
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
