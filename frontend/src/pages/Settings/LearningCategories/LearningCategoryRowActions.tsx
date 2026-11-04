// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { LearningCategory } from '../../../api/routes/learningCategoryRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label, Separator } from '@heroui/react'
import { LuArrowDown, LuArrowUp, LuEllipsis, LuPencil, LuTrash2 } from 'react-icons/lu'

// Misc
import { getLearningCategoryEditUrl } from '../../../urls'

type Props = {
  category: LearningCategory
  // Whether it can move that way among the workspace's own categories.
  canMoveUp: boolean
  canMoveDown: boolean
  onMove: (category: LearningCategory, direction: 'up' | 'down') => void
  onRemove: (category: LearningCategory) => void
}

// The per-row "..." menu of one of the workspace's own categories: edit, move up or down, or
// remove. Built-in categories have none.
export function LearningCategoryRowActions(props: Props) {
  const { t } = useTranslation([ 'learningCategories', 'common' ])

  const actions: Record<string, () => void> = {
    moveUp: () => props.onMove(props.category, 'up'),
    moveDown: () => props.onMove(props.category, 'down'),
    remove: () => props.onRemove(props.category),
  }
  const disabledKeys = [
    ...props.canMoveUp
      ? []
      : [ 'moveUp' ],
    ...props.canMoveDown
      ? []
      : [ 'moveDown' ],
  ]

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
        disabledKeys={disabledKeys}
        onAction={(key: Key) => actions[String(key)]?.()}
      >
        <Dropdown.Item
          id='edit'
          href={getLearningCategoryEditUrl(props.category.key)}
          textValue={t('common:actions.edit')}
        >
          <LuPencil className='size-4' aria-hidden />
          <Label>{t('common:actions.edit')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='moveUp' textValue={t('actions.moveUp')}>
          <LuArrowUp className='size-4' aria-hidden />
          <Label>{t('actions.moveUp')}</Label>
        </Dropdown.Item>
        <Dropdown.Item id='moveDown' textValue={t('actions.moveDown')}>
          <LuArrowDown className='size-4' aria-hidden />
          <Label>{t('actions.moveDown')}</Label>
        </Dropdown.Item>
        <Separator />
        <Dropdown.Item id='remove' textValue={t('actions.remove')} variant='danger'>
          <LuTrash2 className='size-4' aria-hidden />
          <Label>{t('actions.remove')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
