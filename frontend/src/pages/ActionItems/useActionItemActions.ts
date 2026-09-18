// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemTransition } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemDeleted, actionItemUpserted } from '../../store/actionItemsSlice'

// User interface
import { toast } from '@heroui/react'

// Utility
import { HTTPError } from 'ky'

// Misc
import {
  deleteActionItem,
  restoreActionItem,
  snoozeActionItem,
  transitionActionItem,
  waitOnActionItem,
} from '../../api/routes/actionItemRoutes'
import { ACTION_ITEM_PERSON_MAX_CHARACTERS } from '../../constants'
import { useConfirm } from '../../hooks/useConfirm'
import { usePrompt } from '../../hooks/usePrompt'

const transitionToastKeys = {
  accept: 'toasts.accepted',
  resolve: 'toasts.resolved',
  dismiss: 'toasts.dismissed',
  reopen: 'toasts.reopened',
} as const satisfies Record<ActionItemTransition, string>

// What can be done to an item from anywhere it is shown: Next, the inbox, the list, and its
// own page. Each calls the API, puts the answer in Redux at once (the event that follows is
// idempotent), and says what happened. The direct actions resolve to whether they
// succeeded, so a caller can move on only when one did.
export function useActionItemActions() {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()
  const prompt = usePrompt()

  function reportFailure(error: unknown, context: string, item: ActionItem) {
    // The item changed elsewhere since this view drew it, such as resolved in another tab.
    if (error instanceof HTTPError && error.response.status === 409) {
      toast.danger(t('toasts.conflict', { title: item.title }))
      return
    }
    console.debug(`useActionItemActions failed to ${context}`, { error, itemId: item.id })
    toast.danger(t('common:errors.unexpected'))
  }

  async function transition(item: ActionItem, name: ActionItemTransition) {
    try {
      const response = await transitionActionItem(item.id, name)
      dispatch(actionItemUpserted(response.item))
      toast.success(t(transitionToastKeys[name], { title: item.title }))
      return true
    }
    catch (error) {
      reportFailure(error, name, item)
      return false
    }
  }

  // `until` is an instant in the future, or null to end the snooze.
  async function snooze(item: ActionItem, until: string | null) {
    try {
      const response = await snoozeActionItem(item.id, until)
      dispatch(actionItemUpserted(response.item))
      if (until) {
        toast.success(t('toasts.snoozed', { title: item.title }))
      }
      else {
        toast.success(t('toasts.unsnoozed', { title: item.title }))
      }
      return true
    }
    catch (error) {
      reportFailure(error, 'snooze', item)
      return false
    }
  }

  async function stopWaiting(item: ActionItem) {
    try {
      const response = await waitOnActionItem(item.id, null)
      dispatch(actionItemUpserted(response.item))
      toast.success(t('toasts.stoppedWaiting', { title: item.title }))
      return true
    }
    catch (error) {
      reportFailure(error, 'stop waiting', item)
      return false
    }
  }

  // Asks who the next step is waiting on.
  function waitOn(item: ActionItem) {
    prompt({
      title: t('wait.title'),
      label: t('wait.label'),
      description: t('wait.description'),
      defaultValue: item.waitingOn ?? '',
      maxLength: ACTION_ITEM_PERSON_MAX_CHARACTERS,
      submitText: t('wait.submit'),
      onSubmit: async (name) => {
        try {
          const response = await waitOnActionItem(item.id, name)
          dispatch(actionItemUpserted(response.item))
          toast.success(t('toasts.waiting', { title: item.title, name }))
        }
        catch (error) {
          reportFailure(error, 'wait', item)
          // Keeps the dialog open to try again.
          throw error
        }
      },
    })
  }

  // Deleting is soft, so it confirms lightly and says how to undo it.
  function remove(item: ActionItem, onDeleted?: () => void) {
    confirm({
      title: t('delete.title', { title: item.title }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteActionItem(item.id)
        }
        catch (error) {
          reportFailure(error, 'delete', item)
          // Keeps the dialog open to try again.
          throw error
        }
        onDeleted?.()
        dispatch(actionItemDeleted(item.id))
        toast.success(t('toasts.deleted', { title: item.title }))
      },
    })
  }

  async function restore(item: ActionItem) {
    try {
      const response = await restoreActionItem(item.id)
      dispatch(actionItemUpserted(response.item))
      toast.success(t('toasts.restored', { title: item.title }))
      return true
    }
    catch (error) {
      reportFailure(error, 'restore', item)
      return false
    }
  }

  return {
    transition,
    snooze,
    waitOn,
    stopWaiting,
    remove,
    restore,
  } as const
}
