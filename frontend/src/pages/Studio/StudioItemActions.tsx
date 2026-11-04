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
import { Button, Dropdown, Label, Tooltip } from '@heroui/react'
import { LuEllipsis } from 'react-icons/lu'

// Misc
import { getApiErrorMessage } from '../../api/errors'
import { deleteStudioItem } from '../../api/routes/studioRoutes'
import { useConfirm } from '../../hooks/useConfirm'
import { UrlTree } from '../../urls'
import { toast } from '../../toast/toast'

// The delete dialog's one switch, which makes the delete permanent.
const PERMANENT_SWITCH_ID = 'permanently'

type Props = {
  item: StudioItem
}

// The item's menu. Its one action, Delete, confirms first and is soft unless its switch for a
// permanent delete is turned on, which makes the dialog count down as any permanent delete does.
export function StudioItemActions(props: Props) {
  const { t } = useTranslation([ 'studio', 'common' ])
  const dispatch = useAppDispatch()
  const navigate = useNavigate()
  const confirm = useConfirm()
  const item = props.item

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
            console.debug('StudioItemActions failed to delete an item', { error, itemId: item.id })
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
