// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'
import type { PageAction } from '../../layout/page/pageActions'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { studioItemDeleted } from '../../store/studioItemsSlice'

// User interface
import { LuTrash2 } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { deleteStudioItem } from '../../api/routes/studioRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { toast } from '../../toast/toast'
import { UrlTree } from '../../urls'

// The delete dialog's one switch, which makes the delete permanent.
const PERMANENT_SWITCH_ID = 'permanently'

// The item page's actions. Its one action, Delete, confirms first and is soft unless its
// switch for a permanent delete is turned on, which makes the dialog count down as any
// permanent delete does.
export function useStudioItemActions(item: StudioItem): PageAction[] {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()

  function confirmDelete() {
    confirm({
      title: t('delete.title', { title: item.title }),
      message: t('delete.body', { title: item.title }),
      tone: 'danger',
      switches: [{
        id: PERMANENT_SWITCH_ID,
        label: t('delete.permanently'),
        makesIrreversible: true,
      }],
      confirmText: t('delete.confirm'),
      onConfirm: async (switchedOn) => {
        const isPermanent = switchedOn.has(PERMANENT_SWITCH_ID)
        try {
          await deleteStudioItem(item.id, { permanently: isPermanent })
        }
        catch (error) {
          // A provider that refused a file keeps the item, and the answer names the failure.
          const message = getApiErrorMessage(error)
          if (!message) {
            console.debug('useStudioItemActions failed to delete an item', { error, itemId: item.id })
          }
          toast.danger(t('toasts.deleteFailed'), {
            description: message ?? t('common:errors.unexpected'),
          })
          // Keeps the dialog open to try again.
          throw error
        }

        if (isPermanent) {
          dispatch(studioItemDeleted(item.id))
          toast.success(t('toasts.deletedPermanently', { title: item.title }))
          navigate(UrlTree.studio)
          return
        }

        // A soft delete answers with nothing, so the page rereads the item to show it as
        // deleted, with Restore.
        await mutate(`v1/studio-items/${item.id}`)
        toast.success(t('toasts.deleted', { title: item.title }))
      },
    })
  }

  return [{
    id: 'delete',
    label: t('delete.action'),
    icon: LuTrash2,
    isDanger: true,
    onAction: confirmDelete,
  }]
}
