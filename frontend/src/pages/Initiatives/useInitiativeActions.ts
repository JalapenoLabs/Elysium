// Copyright © 2026 Jalapeno Labs

import type { Initiative } from '../../api/routes/initiativeRoutes'

// Core
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { initiativeDeleted, initiativeUpserted } from '../../store/initiativesSlice'

// User interface
import { toast } from '@heroui/react'

// Misc
import { deleteInitiative, restoreInitiative } from '../../api/routes/initiativeRoutes'
import { useConfirm } from '../../hooks/useConfirm'

// Deleting and restoring an initiative, from its page or the list. Both put the answer in
// Redux at once; the events that follow are idempotent.
export function useInitiativeActions() {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const confirm = useConfirm()

  // Deleting is soft: the initiative keeps its items and can be restored.
  function remove(initiative: Initiative, onDeleted?: () => void) {
    confirm({
      title: t('delete.title', { name: initiative.name }),
      message: t('delete.body'),
      tone: 'danger',
      confirmText: t('common:actions.delete'),
      onConfirm: async () => {
        try {
          await deleteInitiative(initiative.id)
        }
        catch (error) {
          console.debug('useInitiativeActions failed to delete', { error, initiativeId: initiative.id })
          toast.danger(t('common:errors.unexpected'))
          // Keeps the dialog open to try again.
          throw error
        }
        onDeleted?.()
        dispatch(initiativeDeleted(initiative.id))
        toast.success(t('toasts.deleted', { name: initiative.name }))
      },
    })
  }

  async function restore(initiative: Initiative) {
    try {
      const response = await restoreInitiative(initiative.id)
      dispatch(initiativeUpserted(response.initiative))
      toast.success(t('toasts.restored', { name: initiative.name }))
    }
    catch (error) {
      console.debug('useInitiativeActions failed to restore', { error, initiativeId: initiative.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  return { remove, restore } as const
}
