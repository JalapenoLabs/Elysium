// Copyright © 2026 Jalapeno Labs

import type { WorkBatchWithHistory } from '../../api/routes/workQueueRoutes'
import type { WorkBatchAction, WorkBatchDecision } from './workQueuePresentation'

// Core
import { useTranslation } from 'react-i18next'
import { HTTPError } from 'ky'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { workBatchLoaded } from '../../store/workQueueSlice'

// Misc
import { getProjectManagerKey } from '../../api/routes/projectManagerRoutes'
import {
  approveWorkBatch,
  cancelWorkBatch,
  getWorkBatchKey,
  rejectWorkBatch,
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
  projectId: string
  title: string
}

// What people do to a waiting batch from the queue, a project's page, or the batch's own page.
// Each lands the batch the API answers in Redux and reloads the queue and the project's lists,
// whose order is the server's, so this tab updates before the event arrives; and each reports its
// own failure. Rejecting and cancelling ask first, for a reason its Project Manager is told,
// since neither can be taken back; rejecting needs one, since it is the person's direction.
export function useWorkBatchActions() {
  const { t } = useTranslation([ 'workQueue', 'common' ])
  const dispatch = useAppDispatch()
  const prompt = usePrompt()

  function landed(batch: ActedOn, response: WorkBatchWithHistory, action: WorkBatchAction) {
    dispatch(workBatchLoaded(response))
    void mutate(WORK_QUEUE_KEY)
    void mutate(getProjectManagerKey(batch.projectId))
    toast.success(t(actionDoneKeys[action], { title: batch.title }))
  }

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

  async function change(batch: ActedOn, action: Exclude<WorkBatchAction, WorkBatchDecision>) {
    try {
      landed(batch, await updateWorkBatch(batch.id, changesByAction[action]), action)
    }
    catch (error) {
      report(error, batch, action)
    }
  }

  async function approve(batch: ActedOn) {
    try {
      landed(batch, await approveWorkBatch(batch.id, {}), 'approve')
    }
    catch (error) {
      report(error, batch, 'approve')
    }
  }

  // Asks for a reason, then sends `decide` with it: what rejecting and cancelling share.
  function askWhy(
    batch: ActedOn,
    action: 'reject' | 'cancel',
    decide: (reason: string) => Promise<WorkBatchWithHistory>,
  ) {
    prompt({
      title: t(`actions.${action}Prompt.title`, { title: batch.title }),
      message: <p>
        <span>{t(`actions.${action}Prompt.message`)} </span>
        <strong>{t(`actions.${action}Prompt.irreversible`)}</strong>
      </p>,
      label: t(`actions.${action}Prompt.reason`),
      isOptional: action === 'cancel',
      isMultiline: true,
      maxLength: REASON_MAX_CHARACTERS,
      submitText: t(`actions.${action}`),
      cancelText: t(`actions.${action}Prompt.keep`),
      onSubmit: async (reason) => {
        try {
          landed(batch, await decide(reason), action)
        }
        catch (error) {
          report(error, batch, action)
          // A batch that moved on cannot be decided by trying again, so the dialog closes.
          if (!(error instanceof HTTPError && error.response.status === 409)) {
            throw error
          }
        }
      },
    })
  }

  return async function act(batch: ActedOn, action: WorkBatchAction) {
    if (action === 'approve') {
      await approve(batch)
      return
    }
    if (action === 'reject') {
      askWhy(batch, action, (reason) => rejectWorkBatch(batch.id, reason))
      return
    }
    if (action === 'cancel') {
      askWhy(batch, action, (reason) => cancelWorkBatch(batch.id, reason || null))
      return
    }
    await change(batch, action)
  }
}
