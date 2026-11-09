// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { undoActionItemChange } from '../../api/routes/actionItemRoutes'
import { toast } from '../../toast/toast'

// Undoes one change to an item's projects or its lead, from its history. The undo's own entry
// arrives on the event stream. A change made again since stays, and the API says why.
export function useUndoActionItemChange() {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()

  return async function undo(itemId: string, entryId: string) {
    try {
      const response = await undoActionItemChange(itemId, entryId)
      dispatch(actionItemUpserted(response.item))
      toast.success(t('history.undone'))
    }
    catch (error) {
      console.debug('useUndoActionItemChange could not undo a change', { error, itemId, entryId })
      toast.warning(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }
}
