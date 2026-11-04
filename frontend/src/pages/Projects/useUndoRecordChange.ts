// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { timelineRecordUpserted } from '../../store/timelineRecordsSlice'

// Misc
import { getApiErrorCode, getApiErrorMessage } from '../../api/errors'
import { recordHistoryKey, undoRecordChange } from '../../api/routes/timelineRoutes'
import { toast } from '../../toast/toast'

// Undoes one change to what a record is about, from a toast's Undo or its history. A change made
// again since stays, and the person is told so rather than shown an error.
export function useUndoRecordChange() {
  const { t } = useTranslation([ 'timeline', 'common' ])
  const dispatch = useAppDispatch()

  return async function undo(recordId: string, entryId: string) {
    try {
      const response = await undoRecordChange(recordId, entryId)
      dispatch(timelineRecordUpserted(response.record))
      void mutate(recordHistoryKey(recordId))
      toast.success(t('attribution.undone'))
    }
    catch (error) {
      console.debug('useUndoRecordChange could not undo a change', { error, recordId, entryId })
      if (getApiErrorCode(error) === 'attribution_changed') {
        toast.warning(t('attribution.changedSince'))
        return
      }
      toast.danger(getApiErrorMessage(error) ?? t('common:errors.unexpected'))
    }
  }
}
