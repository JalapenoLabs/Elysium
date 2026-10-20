// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { studioItemUpserted } from '../../store/studioItemsSlice'

// User interface
import { Button, toast } from '@heroui/react'
import { LuUndo2 } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { restoreStudioItem } from '../../api/routes/studioRoutes'

type Props = {
  item: StudioItem
}

// Brings a softly deleted item back to the grid, with its files and conversation.
export function RestoreStudioItemButton(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const [ isRestoring, setIsRestoring ] = useState(false)

  async function restore() {
    setIsRestoring(true)
    try {
      const response = await restoreStudioItem(props.item.id)
      dispatch(studioItemUpserted(response.item))
      toast.success(t('toasts.restored', { title: response.item.title }))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('RestoreStudioItemButton failed to restore an item', { error, itemId: props.item.id })
      }
      toast.danger(t('toasts.restoreFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
    finally {
      setIsRestoring(false)
    }
  }

  return <Button
    size='sm'
    variant='outline'
    isPending={isRestoring}
    onPress={() => void restore()}
  >
    <LuUndo2 className='size-4' aria-hidden />
    <span>{t('grid.restore')}</span>
  </Button>
}
