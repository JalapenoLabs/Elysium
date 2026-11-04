// Copyright © 2026 Jalapeno Labs

import type { WorkBatchAction } from './workQueuePresentation'

// Core
import { useTranslation } from 'react-i18next'
import { HTTPError } from 'ky'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { workBatchLoaded } from '../../store/workQueueSlice'

// Misc
import {
  cancelWorkBatch,
  getWorkBatchKey,
  updateWorkBatch,
  WORK_QUEUE_KEY,
} from '../../api/routes/workQueueRoutes'
import { usePrompt } from '../../hooks/usePrompt'
import { toast } from '../../toast/toast'
import { actionDoneKeys, changesByAction } from './workQueuePresentation'

// The most characters a reason holds, matching `work_batches.result`.
const REASON_MAX_CHARACTERS = 20_000

type ActedOn = {
  id: string
  title: string
}

// What people do to a waiting batch from the queue or its page. Each lands the batch the API
// answers in Redux and reloads the queue, whose order is the server's, so this tab updates before
// the event arrives; and each reports its own failure. Cancelling asks first, for a reason its
// Project Manager is told, since it can't be taken back.
export function useWorkBatchActions() {
  const { t } = useTranslation([ 'workQueue', 'common' ])
  const dispatch = useAppDispatch()
  const prompt = usePrompt()

  function report(error: unknown, batch: ActedOn, action: WorkBatchAction) {
    // The batch moved on in the engine or another tab since this page drew it: show it as it is.
    if (error instanceof HTTPError && error.response.status === 409) {
      toast.danger(t('actions.conflict'))
      void mutate(WORK_QUEUE_KEY)
      void mutate(getWorkBatchKey(batch.id))
      return
    }
    console.debug(`useWorkBatchActions could not ${action} a batch`, { error, batchId: batch.id })
    toast.danger(t('common:errors.unexpected'))
  }

  async function change(batch: ActedOn, action: Exclude<WorkBatchAction, 'cancel'>) {
    try {
      const response = await updateWorkBatch(batch.id, changesByAction[action])
      dispatch(workBatchLoaded(response))
      void mutate(WORK_QUEUE_KEY)
      toast.success(t(actionDoneKeys[action], { title: batch.title }))
    }
    catch (error) {
      report(error, batch, action)
    }
  }

  function cancel(batch: ActedOn) {
    prompt({
      title: t('actions.cancelPrompt.title', { title: batch.title }),
      message: <p>
        <span>{t('actions.cancelPrompt.message')} </span>
        <strong>{t('actions.cancelPrompt.irreversible')}</strong>
      </p>,
      label: t('actions.cancelPrompt.reason'),
      isOptional: true,
      isMultiline: true,
      maxLength: REASON_MAX_CHARACTERS,
      submitText: t('actions.cancel'),
      cancelText: t('actions.cancelPrompt.keep'),
      onSubmit: async (reason) => {
        try {
          const response = await cancelWorkBatch(batch.id, reason || null)
          dispatch(workBatchLoaded(response))
          void mutate(WORK_QUEUE_KEY)
          toast.success(t(actionDoneKeys.cancel, { title: batch.title }))
        }
        catch (error) {
          report(error, batch, 'cancel')
          // A batch that moved on cannot be cancelled by trying again, so the dialog closes.
          if (!(error instanceof HTTPError && error.response.status === 409)) {
            throw error
          }
        }
      },
    })
  }

  return async function act(batch: ActedOn, action: WorkBatchAction) {
    if (action === 'cancel') {
      cancel(batch)
      return
    }
    await change(batch, action)
  }
}
