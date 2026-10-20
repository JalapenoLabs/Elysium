// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { StudioItem } from '../../api/routes/studioRoutes'

// Core
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'
import { mutate } from 'swr'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { studioItemDeleted } from '../../store/studioItemsSlice'

// User interface
import { Button, Dropdown, Label, toast, Tooltip } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'
import { DeleteStudioItemMessage } from './DeleteStudioItemMessage'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { deleteStudioItem } from '../../api/routes/studioRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { UrlTree } from '../../urls'

type Props = {
  item: StudioItem
}

// The item's menu. Its one action, Delete, confirms first and is soft unless the box for a
// permanent delete is checked.
export function StudioItemActions(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()
  const item = props.item

  function confirmDelete() {
    // The box lives in the dialog; this holds its last state for the confirm to read.
    const choice = { isPermanent: false }

    confirm({
      title: t('delete.title', { title: item.title }),
      message: <DeleteStudioItemMessage
        onPermanentChange={(isPermanent) => {
          choice.isPermanent = isPermanent
        }}
      />,
      tone: 'danger',
      confirmText: t('delete.confirm'),
      onConfirm: async () => {
        try {
          await deleteStudioItem(item.id, { permanently: choice.isPermanent })
        }
        catch (error) {
          // A provider that refused a file keeps the item, and the answer names the failure.
          const message = getApiErrorMessage(error)
          if (!message) {
            console.debug('StudioItemActions failed to delete an item', { error, itemId: item.id })
          }
          toast.danger(t('toasts.deleteFailed'), {
            description: message ?? t('common:errors.unexpected'),
          })
          // Keeps the dialog open to try again.
          throw error
        }

        if (choice.isPermanent) {
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

  const actions: Record<string, () => void> = {
    delete: confirmDelete,
  }

  return <Dropdown>
    <Tooltip delay={300}>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        aria-label={t('item.actions')}
      >
        <LuEllipsis className='size-4' aria-hidden />
      </Button>
      <Tooltip.Content>
        <span>{t('item.actions')}</span>
      </Tooltip.Content>
    </Tooltip>
    <Dropdown.Popover placement='bottom end'>
      <Dropdown.Menu onAction={(key: Key) => actions[String(key)]?.()}>
        <Dropdown.Item id='delete' textValue={t('delete.action')} variant='danger'>
          <Label>{t('delete.action')}</Label>
        </Dropdown.Item>
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>
}
