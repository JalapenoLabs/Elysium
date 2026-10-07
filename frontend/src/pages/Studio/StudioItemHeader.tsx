// Copyright © 2026 Jalapeno Labs

import type { ReactNode } from 'react'
import type { StudioItem } from '../../api/routes/studioRoutes'
import type { StudioLayoutMode } from './studioLayout'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { studioItemUpserted } from '../../store/studioItemsSlice'

// User interface
import { Button, toast, Tooltip } from '@heroui/react'
import { LuArrowLeft } from 'react-icons/lu'
import { InlineEditableText } from '../../components/InlineEditableText'
import { StudioLayoutControl } from './StudioLayoutControl'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { updateStudioItem } from '../../api/routes/studioRoutes'
import { UrlTree } from '../../urls'
import { STUDIO_TITLE_MAX_CHARACTERS } from './studioItemFormSchema'

type Props = {
  item: StudioItem
  isReadOnly: boolean
  layoutMode: StudioLayoutMode
  onLayoutModeChange: (mode: StudioLayoutMode) => void
  // The item's actions, such as Delete.
  actions: ReactNode
}

// The item page's top row: back to the grid, the title (renamed in place), which columns
// show, and the item's actions.
export function StudioItemHeader(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()

  async function rename(title: string) {
    try {
      const response = await updateStudioItem(props.item.id, { title })
      dispatch(studioItemUpserted(response.item))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('StudioItemHeader failed to rename an item', { error, itemId: props.item.id })
      }
      toast.danger(t('toasts.renameFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
      // Keeps the editor open on what was typed.
      throw error
    }
  }

  return <div className='flex shrink-0 flex-wrap items-center gap-3 border-b border-separator px-4 py-2'>
    <Tooltip delay={300}>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        aria-label={t('item.back')}
        onPress={() => navigate(UrlTree.studio)}
      >
        <LuArrowLeft className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{t('item.back')}</span>
      </Tooltip.Content>
    </Tooltip>

    <div className='min-w-0 flex-1'>{
      props.isReadOnly
        ? <h1 className='truncate text-lg font-semibold'>{props.item.title}</h1>
        : <InlineEditableText
          value={props.item.title}
          label={t('item.renameLabel')}
          isRequired
          maxLength={STUDIO_TITLE_MAX_CHARACTERS}
          className='text-lg font-semibold'
          onSave={rename}
        />
    }</div>

    <StudioLayoutControl
      mode={props.layoutMode}
      onChange={props.onLayoutModeChange}
    />
    {props.actions}
  </div>
}
