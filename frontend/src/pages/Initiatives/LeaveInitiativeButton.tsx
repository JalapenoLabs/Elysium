// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../../store/hooks'
import { actionItemUpserted } from '../../store/actionItemsSlice'

// User interface
import { Button, toast, Tooltip } from '@heroui/react'
import { LuX } from 'react-icons/lu'

// Misc
import { leaveInitiative } from '../../api/routes/actionItemRoutes'

type Props = {
  item: ActionItem
  initiativeId: string
}

// Takes an item out of an initiative. The item itself stays, and its time in the
// initiative stays in the burnup's history.
export function LeaveInitiativeButton(props: Props) {
  const { t } = useTranslation([ 'initiatives', 'common' ])
  const dispatch = useAppDispatch()
  const [ isLeaving, setIsLeaving ] = useState(false)

  async function leave() {
    setIsLeaving(true)
    try {
      const response = await leaveInitiative(props.item.id, props.initiativeId)
      dispatch(actionItemUpserted(response.item))
    }
    catch (error) {
      console.debug('LeaveInitiativeButton failed to remove an item', { error, itemId: props.item.id })
      toast.danger(t('common:errors.unexpected'))
    }
    finally {
      setIsLeaving(false)
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
