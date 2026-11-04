// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'
import type { PageAction } from '../../layout/page/pageActions'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { studioItemDeleted, studioItemUpserted } from '../../store/studioItemsSlice'

// User interface
import { LuPencil, LuTrash2 } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { deleteStudioItem, updateStudioItem } from '../../api/routes/studioRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { usePrompt } from '../../hooks/usePrompt'
import { STUDIO_TITLE_MAX_CHARACTERS } from './studioItemFormSchema'
import { toast } from '../../toast/toast'
import { UrlTree } from '../../urls'

// The delete dialog's one switch, which makes the delete permanent.
const PERMANENT_SWITCH_ID = 'permanently'

// The item page's actions. Rename asks for the new title in a dialog, saved by its own
// button: the page is a workspace with no Save of its own. Delete confirms first and is soft
// unless its switch for a permanent delete is turned on, which makes the dialog count down as
// any permanent delete does. A deleted item can only be deleted for good.
export function useStudioItemActions(item: StudioItem): PageAction[] {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()
  const prompt = usePrompt()

  function promptRename() {
    prompt({
      title: t('item.renameTitle'),
      label: t('item.renameLabel'),
      defaultValue: item.title,
      maxLength: STUDIO_TITLE_MAX_CHARACTERS,
      onSubmit: async (title) => {
        try {
          const response = await updateStudioItem(item.id, { title })
          dispatch(studioItemUpserted(response.item))
          toast.success(t('toasts.renamed', { title: response.item.title }))
        }
        catch (error) {
          const message = getApiErrorMessage(error)
          if (!message) {
            console.debug('useStudioItemActions failed to rename an item', { error, itemId: item.id })
          }
          toast.danger(t('toasts.renameFailed'), {
            description: message ?? t('common:errors.unexpected'),
          })
          // Keeps the dialog open on what was typed.
          throw error
        }
      },
    })
  }

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

  const remove: PageAction = {
    id: 'delete',
    label: t('delete.action'),
    icon: LuTrash2,
    isDanger: true,
    onAction: confirmDelete,
  }
  if (item.deletedAt) {
    return [ remove ]
  }
  return [
    { id: 'rename', label: t('item.rename'), icon: LuPencil, onAction: promptRename },
    remove,
  ]
}
