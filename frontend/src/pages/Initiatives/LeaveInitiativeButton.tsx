// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'

// User interface
import { Button, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'

// Misc
import { joinInitiative, leaveInitiative } from '../../api/routes/actionItemRoutes'
import { toast } from '../../toast/toast'

type Props = {
  item: ActionItem
  initiativeId: string
}

// Takes an item out of an initiative. The item itself stays, and its time in the
// initiative stays in the burnup's history. Joining again puts it back, so it happens at once,
// with Undo in its toast.
export function LeaveInitiativeButton(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const [ isLeaving, setIsLeaving ] = useState(false)
  const item = props.item

  async function leave() {
    setIsLeaving(true)
    try {
      const response = await leaveInitiative(item.id, props.initiativeId)
      dispatch(actionItemUpserted(response.item))
      toast.success(t('toasts.itemTakenOut', { title: item.title }), {
        action: {
          label: t('common:actions.undo'),
          onPress: () => void putBack(),
        },
      })
    }
    catch (error) {
      console.debug('LeaveInitiativeButton failed to remove an item', { error, itemId: item.id })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsLeaving(false)
    }
  }

  // The button is gone by now, with its row, so this reports through toasts alone.
  async function putBack() {
    try {
      const response = await joinInitiative(item.id, props.initiativeId)
      dispatch(actionItemUpserted(response.item))
      toast.success(t('toasts.itemPutBack', { title: item.title }))
    }
    catch (error) {
      console.debug('LeaveInitiativeButton failed to put an item back', { error, itemId: item.id })
      toast.danger(t('common:errors.unexpected'))
    }
  }

  return <Tooltip delay={300}>
    <div>
      <Button
        isIconOnly
        size='sm'
        variant='ghost'
        aria-label={t('members.remove')}
        isPending={isLeaving}
        onPress={() => void leave()}
      >
        <LuX className='size-4' aria-hidden />
      </Button>
    </div>
    <Tooltip.Content>
      <span>{t('members.remove')}</span>
    </Tooltip.Content>
  </Tooltip>
}
