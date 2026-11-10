// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemTransition } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemDeleted, actionItemUpserted } from '../../store/actionItemsSlice'

// Utility
import { HTTPError } from 'ky'

// Misc
import {
  assignActionItemToManager,
  deleteActionItem,
  getManagerAssignmentKey,
  restoreActionItem,
  snoozeActionItem,
  transitionActionItem,
  unassignActionItemFromManager,
  waitOnActionItem,
} from '../../api/routes/actionItemRoutes'
import { ACTION_ITEM_PERSON_MAX_CHARACTERS } from '../../constants'
import { usePrompt } from '../../hooks/usePrompt'
import { LEAVE_WITHOUT_ASKING } from '../../layout/page/pageSave'
import { toast } from '../../toast/toast'
import { getActionItemViewUrl } from '../../urls'

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
  const navigate = useNavigate()
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

  // Wakes the item's lead project's Project Manager to plan it; what it plans waits for the
  // person's approval. Taking it back leaves what was planned for the person to decide. Unlike
  // the other actions, an item page alone offers these, so the plan it shows is read again.
  async function assignToManager(item: ActionItem) {
    try {
      const response = await assignActionItemToManager(item.id)
      dispatch(actionItemUpserted(response.item))
      void mutate(getManagerAssignmentKey(item.id))
      toast.success(t('toasts.assignedToManager', { title: item.title }))
      return true
    }
    catch (error) {
      reportFailure(error, 'assign to its Project Manager', item)
      return false
    }
  }

  async function unassignFromManager(item: ActionItem) {
    try {
      const response = await unassignActionItemFromManager(item.id)
      dispatch(actionItemUpserted(response.item))
      void mutate(getManagerAssignmentKey(item.id))
      toast.success(t('toasts.unassignedFromManager', { title: item.title }))
      return true
    }
    catch (error) {
      reportFailure(error, 'take back from its Project Manager', item)
      return false
    }
  }

  // Asks who the next step is waiting on.
  function waitOn(item: ActionItem) {
    prompt({
      title: t('wait.title'),
      label: t('wait.label'),
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

  // Deleting is soft, so it happens at once, with Undo in its toast. Deleted from its own page,
  // the item is left for `leaveTo`, and Undo brings the person back to it.
  async function remove(item: ActionItem, leaveTo?: string) {
    try {
      await deleteActionItem(item.id)
    }
    catch (error) {
      reportFailure(error, 'delete', item)
      return false
    }

    // Leave first, so the item's page never renders it as missing. Deleting it drops its
    // unsaved edits too, so leaving asks nothing.
    if (leaveTo) {
      void navigate(leaveTo, { state: LEAVE_WITHOUT_ASKING })
    }
    dispatch(actionItemDeleted(item.id))
    toast.success(t('toasts.deleted', { title: item.title }), {
      action: {
        label: t('common:actions.undo'),
        onPress: () => void undoRemove(item, Boolean(leaveTo)),
      },
    })
    return true
  }

  async function undoRemove(item: ActionItem, isReturning: boolean) {
    const isRestored = await restore(item)
    if (isRestored && isReturning) {
      void navigate(getActionItemViewUrl(item.id))
    }
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
    assignToManager,
    unassignFromManager,
  } as const
}
