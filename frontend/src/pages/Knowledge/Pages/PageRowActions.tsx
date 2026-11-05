// Copyright © 2026 Jalapeno Labs

import type { KnowledgePageSummary } from '../../../api/routes/pageRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Header, Label, Separator, Tooltip } from '@heroui/react'
import { LuEllipsis, LuFilePlus, LuFolderInput, LuTrash2 } from 'react-icons/lu'

// Misc
import { getNewChildPageUrl } from '../../../urls'

type Props = {
  page: KnowledgePageSummary
  onMove: (page: KnowledgePageSummary) => void
  onDelete: (page: KnowledgePageSummary) => void
  // Following the link: the phone's drawer closes.
  onNavigate?: () => void
}

// A row's "..." menu. A page Elysium relies on still takes children, but stays where it is, and the
// menu says so in its own text: a disabled item hears no hover, and a touch screen has none.
export function PageRowActions(props: Props) {
  const { t } = useTranslation('knowledge')
  const page = props.page
  const label = t('pages.actions.label', { title: page.title })

  const moveItem = <Dropdown.Item
    id='move'
    textValue={t('pages.actions.move')}
    isDisabled={page.fixed}
    onAction={() => props.onMove(page)}
  >
    <LuFolderInput className='size-4' aria-hidden />
    <Label>{t('pages.actions.move')}</Label>
  </Dropdown.Item>

  const deleteItem = <Dropdown.Item
    id='delete'
    textValue={t('pages.actions.delete')}
    variant='danger'
    isDisabled={page.fixed}
    onAction={() => props.onDelete(page)}
  >
    <LuTrash2 className='size-4' aria-hidden />
    <Label>{t('pages.actions.delete')}</Label>
  </Dropdown.Item>

  return <Dropdown>
    <Tooltip delay={300}>
      <Button isIconOnly size='sm' variant='ghost' aria-label={label} className='size-6 min-w-0 pointer-coarse:size-8'>
        <LuEllipsis className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{label}</span>
      </Tooltip.Content>
    </Tooltip>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu aria-label={label}>
        <Dropdown.Item
          id='add'
          textValue={t('pages.actions.addChild')}
          href={getNewChildPageUrl(page.id)}
          onAction={props.onNavigate}
        >
          <LuFilePlus className='size-4' aria-hidden />
          <Label>{t('pages.actions.addChild')}</Label>
        </Dropdown.Item>
        {page.fixed
          ? <Dropdown.Section>
            <Header>{t('pages.actions.fixed')}</Header>
            {moveItem}
            {deleteItem}
          </Dropdown.Section>
          : moveItem}
        {!page.fixed && <Separator />}
        {!page.fixed && deleteItem}
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
