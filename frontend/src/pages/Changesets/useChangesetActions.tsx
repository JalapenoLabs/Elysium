// Copyright © 2026 Jalapeno Labs

import type { Changeset, ChangesetDecision } from '../../api/routes/changesetRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { HTTPError } from 'ky'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { changesetUpserted } from '../../store/changesetsSlice'

// User interface
import { toast } from '@heroui/react'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { applyChangeset, decideChangeset, undoChangeset } from '../../api/routes/changesetRoutes'
import { useConfirm } from '../../hooks/useConfirm'

// The writes the review page makes. Each dispatches the changeset the API answers, so this
// tab updates before the event arrives, and reports its own failure.
export function useChangesetActions() {
  const { t } = useTranslation([ 'changesets', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()

  function reportFailure(error: unknown, context: string, changeset: Changeset) {
    // Decided, applied, or undone in another tab since this page drew it: reload it.
    if (error instanceof HTTPError && error.response.status === 409) {
      toast.danger(t('toasts.conflict'))
      void mutate(`v1/changesets/${changeset.id}`)
      return
    }
    // A refusal the API explains, such as approving a change whose dependency is rejected.
    const message = error instanceof HTTPError && error.response.status === 400
      ? getApiErrorMessage(error)
      : null
    console.debug(`useChangesetActions failed to ${context}`, { error, changesetId: changeset.id })
    toast.danger(message ?? t('common:errors.unexpected'))
  }

  // Decides the operations `operationIds` names, or every one when it is left out.
  async function decide(changeset: Changeset, decision: ChangesetDecision, operationIds?: string[]) {
    try {
      const response = await decideChangeset(changeset.id, { decision, operationIds })
      dispatch(changesetUpserted(response.changeset))
    }
    catch (error) {
      reportFailure(error, 'decide', changeset)
    }
  }

  async function apply(changeset: Changeset) {
    try {
      const response = await applyChangeset(changeset.id)
      dispatch(changesetUpserted(response.changeset))
      if (response.changeset.state === 'rejected') {
        toast.success(t('toasts.rejected'))
        return
      }
      toast.success(t('toasts.applied'))
    }
    catch (error) {
      reportFailure(error, 'apply', changeset)
    }
  }

  // Asks first, saying what undo cannot take back.
  function undo(changeset: Changeset) {
    confirm({
      title: t('review.undoTitle'),
      message: <>
        <p className='compact'>{t('review.undoBody')}</p>
        <p className='text-sm opacity-80'>{t('review.undoLimits')}</p>
      </>,
      confirmText: t('review.undo'),
      tone: 'danger',
      onConfirm: async () => {
        try {
          const response = await undoChangeset(changeset.id)
          dispatch(changesetUpserted(response.changeset))
          toast.success(t('toasts.undone'))
        }
        catch (error) {
          reportFailure(error, 'undo', changeset)
          throw error
        }
      },
    })
  }

  return { decide, apply, undo } as const
}
