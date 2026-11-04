// Copyright © 2026 Jalapeno Labs

import type { StudioItem } from '../../api/routes/studioRoutes'
import type { PageAction } from '../../layout/page/pageActions'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { studioItemDeleted, studioItemUpserted } from '../../store/studioItemsSlice'

// User interface
import { LuPencil, LuTrash2 } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { deleteStudioItem, restoreStudioItem, updateStudioItem } from '../../api/routes/studioRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { usePrompt } from '../../hooks/usePrompt'
import { STUDIO_TITLE_MAX_CHARACTERS } from './studioItemFormSchema'
import { toast } from '../../toast/toast'
import { getStudioItemUrl, UrlTree } from '../../urls'

// The item page's actions. Rename asks for the new title in a dialog, saved by its own
// button: the page is a workspace with no Save of its own. Delete is soft, so it happens at
// once, with Undo in its toast: the page leaves for the grid, and Undo comes back. Delete
// permanently confirms first and counts down, as any permanent delete does. A deleted item can
// only be deleted for good.
export function useStudioItemActions(item: StudioItem): PageAction[] {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()
  const prompt = usePrompt()
  const [ isDeleting, setIsDeleting ] = useState(false)

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

  // A provider that refused a file keeps the item, and the answer names the failure.
  function reportDeleteFailure(error: unknown) {
    const message = getApiErrorMessage(error)
    if (!message) {
      console.debug('useStudioItemActions failed to delete an item', { error, itemId: item.id })
    }
    toast.danger(t('toasts.deleteFailed'), {
      description: message ?? t('common:errors.unexpected'),
    })
  }

  async function remove() {
    setIsDeleting(true)
    try {
      await deleteStudioItem(item.id, { permanently: false })
    }
    catch (error) {
      reportDeleteFailure(error)
      return
    }
    finally {
      setIsDeleting(false)
    }

    // Leave first, so this page never renders the item as missing.
    void navigate(UrlTree.studio)
    dispatch(studioItemDeleted(item.id))
    toast.success(t('toasts.deleted', { title: item.title }), {
      action: {
        label: t('common:actions.undo'),
        onPress: () => void undoRemove(),
      },
    })
  }

  // The page is gone by now, so this reports through toasts alone, and comes back to the item.
  async function undoRemove() {
    try {
      const response = await restoreStudioItem(item.id)
      dispatch(studioItemUpserted(response.item))
      toast.success(t('toasts.restored', { title: response.item.title }))
      void navigate(getStudioItemUrl(item.id))
    }
    catch (error) {
      const message = getApiErrorMessage(error)
      if (!message) {
        console.debug('useStudioItemActions failed to restore an item', { error, itemId: item.id })
      }
      toast.danger(t('toasts.restoreFailed'), {
        description: message ?? t('common:errors.unexpected'),
      })
    }
  }

  function confirmPermanentDelete() {
    confirm({
      title: t('delete.permanentlyTitle', { title: item.title }),
      message: t('delete.permanentlyBody', { title: item.title }),
      isIrreversible: true,
      confirmText: t('delete.permanentlyConfirm'),
      onConfirm: async () => {
        try {
          await deleteStudioItem(item.id, { permanently: true })
        }
        catch (error) {
          reportDeleteFailure(error)
          // Keeps the dialog open to try again.
          throw error
        }
        void navigate(UrlTree.studio)
        dispatch(studioItemDeleted(item.id))
        toast.success(t('toasts.deletedPermanently', { title: item.title }))
      },
    })
  }

  const removePermanently: PageAction = {
    id: 'deletePermanently',
    label: t('delete.permanentlyAction'),
    icon: LuTrash2,
    isDanger: true,
    onAction: confirmPermanentDelete,
  }
  if (item.deletedAt) {
    return [ removePermanently ]
  }
  return [
    { id: 'rename', label: t('item.rename'), icon: LuPencil, onAction: promptRename },
    {
      id: 'delete',
      label: t('delete.action'),
      icon: LuTrash2,
      isDanger: true,
      isPending: isDeleting,
      onAction: () => void remove(),
    },
    removePermanently,
  ]
}
