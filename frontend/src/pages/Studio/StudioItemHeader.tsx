// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'
import type { StudioLayoutMode } from './studioLayout'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { studioItemUpserted } from '../../store/studioItemsSlice'

// User interface
import { PageHeader } from '../../layout/page/PageHeader'
import { StudioLayoutControl } from './StudioLayoutControl'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { updateStudioItem } from '../../api/routes/studioRoutes'
import { UrlTree } from '../../urls'
import { STUDIO_TITLE_MAX_CHARACTERS } from './studioItemFormSchema'
import { useStudioItemActions } from './useStudioItemActions'
import { toast } from '../../toast/toast'

type Props = {
  item: StudioItem
  isReadOnly: boolean
  layoutMode: StudioLayoutMode
  onLayoutModeChange: (mode: StudioLayoutMode) => void
}

// The item page's top bar, the standard page header: back to the grid, the title (renamed
// in place), which columns show, and the item's actions.
export function StudioItemHeader(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const actions = useStudioItemActions(props.item)

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

  return <PageHeader
    className='shrink-0 border-b border-separator px-4 py-3'
    title={props.item.title}
    rename={props.isReadOnly
      ? undefined
      : { label: t('item.renameLabel'), maxLength: STUDIO_TITLE_MAX_CHARACTERS, onSave: rename }}
    parents={[{ label: t('title'), href: UrlTree.studio }]}
    tools={<StudioLayoutControl
      mode={props.layoutMode}
      onChange={props.onLayoutModeChange}
    />}
    actions={actions}
  />
}
