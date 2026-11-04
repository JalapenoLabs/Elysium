// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { initiativeDeleted, initiativeUpserted } from '../../store/initiativesSlice'

// Misc
import { deleteInitiative, restoreInitiative } from '../../api/routes/initiativeRoutes'
import { LEAVE_WITHOUT_ASKING } from '../../layout/page/pageSave'
import { toast } from '../../toast/toast'
import { getInitiativeViewUrl } from '../../urls'

// Deleting and restoring an initiative, from its page or the list. Both put the answer in
// Redux at once; the events that follow are idempotent.
export function useInitiativeActions() {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()

  // Deleting is soft: the initiative keeps its items and can be restored, so it happens at once,
  // with Undo in its toast. Deleted from its own page, the initiative is left for `leaveTo`, and
  // Undo brings the person back to it.
  async function remove(initiative: Initiative, leaveTo?: string) {
    try {
      await deleteInitiative(initiative.id)
    }
    catch (error) {
      console.debug('useInitiativeActions failed to delete', { error, initiativeId: initiative.id })
      toast.danger(t('common:errors.unexpected'))
      return false
    }

    // Leave first, so the initiative's page never renders it as missing. Deleting it drops its
    // unsaved edits too, so leaving asks nothing.
    if (leaveTo) {
      void navigate(leaveTo, { state: LEAVE_WITHOUT_ASKING })
    }
    dispatch(initiativeDeleted(initiative.id))
    toast.success(t('toasts.deleted', { name: initiative.name }), {
      action: {
        label: t('common:actions.undo'),
        onPress: () => void undoRemove(initiative, Boolean(leaveTo)),
      },
    })
    return true
  }

  async function undoRemove(initiative: Initiative, isReturning: boolean) {
    const isRestored = await restore(initiative)
    if (isRestored && isReturning) {
      void navigate(getInitiativeViewUrl(initiative.id))
    }
  }

  async function restore(initiative: Initiative) {
    try {
      const response = await restoreInitiative(initiative.id)
      dispatch(initiativeUpserted(response.initiative))
      toast.success(t('toasts.restored', { name: initiative.name }))
      return true
    }
    catch (error) {
      console.debug('useInitiativeActions failed to restore', { error, initiativeId: initiative.id })
      toast.danger(t('common:errors.unexpected'))
      return false
    }
  }

  return { remove, restore } as const
}
