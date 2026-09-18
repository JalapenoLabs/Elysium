// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// User interface
import { Button, Dropdown, Label } from '@heroui/react'
import { LuAlarmClock, LuEllipsis, LuHourglass, LuSquareTerminal, LuTrash2 } from 'react-icons/lu'
import { SnoozeMenu } from './SnoozeMenu'

// Misc
import { getNewCodingSessionUrl, UrlTree } from '../../urls'
import { transitionLabelKeys, transitionsByState } from './actionItemPresentation'
import { useActionItemActions } from './useActionItemActions'

type Props = {
  item: ActionItem
  now: number
}

// The actions at the top of an item's page: the state changes its state allows, snoozing,
// waiting on someone, starting a coding session from it, and, in a menu, deleting. The
// first allowed state change leads.
export function ActionItemActionBar(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const navigate = useNavigate()
  const actions = useActionItemActions()
  const [ isSnoozeOpen, setIsSnoozeOpen ] = useState(false)
  const item = props.item
  const isClosed = item.state === 'resolved' || item.state === 'dismissed'

  const menuActions: Record<string, () => void> = {
    stopWaiting: () => void actions.stopWaiting(item),
    // Leave first, so this page never renders the item as missing.
    delete: () => actions.remove(item, () => navigate(UrlTree.actionItemsAll)),
  }

  return <div className='flex flex-wrap items-center gap-2'>
    {transitionsByState[item.state].map((transition, index) => <Button
      key={transition}
      size='sm'
      variant={index === 0
        ? 'primary'
        : 'outline'}
      onPress={() => void actions.transition(item, transition)}
    >
      <span>{t(transitionLabelKeys[transition])}</span>
    </Button>)}

    {!isClosed && <SnoozeMenu
      size='sm'
      item={item}
      now={props.now}
      isOpen={isSnoozeOpen}
      onOpenChange={setIsSnoozeOpen}
      onSnooze={(until) => actions.snooze(item, until)}
    >
      <LuAlarmClock className='size-4' aria-hidden />
      <span>{t('snooze.label')}</span>
    </SnoozeMenu>}

    {!isClosed && <Button size='sm' variant='outline' onPress={() => actions.waitOn(item)}>
      <LuHourglass className='size-4' aria-hidden />
      <span>{item.waitingOn
        ? t('wait.change')
        : t('wait.action')}</span>
    </Button>}

    <Button size='sm' variant='outline' onPress={() => navigate(getNewCodingSessionUrl(item.id))}>
      <LuSquareTerminal className='size-4' aria-hidden />
      <span>{t('sessions.start')}</span>
    </Button>

    <Dropdown>
      <Button isIconOnly size='sm' variant='ghost' aria-label={t('common:actions.moreActions')}>
        <LuEllipsis className='size-4' aria-hidden />
      </Button>
      <Dropdown.Popover placement='bottom end'>
        <Dropdown.Menu
          aria-label={t('common:actions.moreActions')}
          disabledKeys={item.waitingOn
            ? []
            : [ 'stopWaiting' ]}
          onAction={(key: Key) => menuActions[String(key)]?.()}
        >
          <Dropdown.Item id='stopWaiting' textValue={t('wait.stop')}>
            <Label>{t('wait.stop')}</Label>
          </Dropdown.Item>
          <Dropdown.Item id='delete' textValue={t('common:actions.delete')} variant='danger'>
            <LuTrash2 className='size-4' aria-hidden />
            <Label>{t('common:actions.delete')}</Label>
          </Dropdown.Item>
        </Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>
  </div>
}
