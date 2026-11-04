// Copyright © 2026 Jalapeno Labs

import type { IconType } from 'react-icons'
import type { ActionItem, ActionItemTransition } from '../../api/routes/actionItemRoutes'
import type { PageAction } from '../../layout/page/pageActions'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import {
  LuAlarmClock,
  LuAlarmClockOff,
  LuCalendarDays,
  LuCheck,
  LuHourglass,
  LuInbox,
  LuRotateCcw,
  LuSquareTerminal,
  LuTimerOff,
  LuTrash2,
  LuX,
} from 'react-icons/lu'

// Utility
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { getNewCodingSessionUrl, UrlTree } from '../../urls'
import { getSnoozePresetInstants, SNOOZE_PRESETS } from './actionItemDates'
import {
  isSnoozed,
  snoozePresetIcons,
  snoozePresetLabelKeys,
  transitionLabelKeys,
  transitionsByState,
} from './actionItemPresentation'
import { useActionItemActions } from './useActionItemActions'

const transitionIcons = {
  accept: LuInbox,
  resolve: LuCheck,
  dismiss: LuX,
  reopen: LuRotateCcw,
} as const satisfies Record<ActionItemTransition, IconType>

type Options = {
  item: ActionItem
  now: number
  // Opens the dialog that snoozes until a picked day, which the page holds.
  onPickSnoozeDate: () => void
}

// What an item's page can do, for its header. The first state change its state allows
// leads as the page's one primary action; the rest, snoozing, waiting on someone, starting
// a coding session, and deleting, are its Actions menu.
export function useActionItemPageActions(options: Options) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const actions = useActionItemActions()
  // The write in flight, so its button or the menu's trigger shows it working.
  const [ pendingId, setPendingId ] = useState<string | null>(null)
  const item = options.item
  const isClosed = item.state === 'resolved' || item.state === 'dismissed'

  async function run(id: string, write: () => Promise<boolean>) {
    setPendingId(id)
    try {
      await write()
    }
    finally {
      setPendingId(null)
    }
  }

  const [ leadTransition, ...otherTransitions ] = transitionsByState[item.state]
  // Always one that acts, so the lead can be drawn as a plain button.
  function transitionAction(transition: ActionItemTransition): Extract<PageAction, { onAction: () => void }> {
    return {
      id: transition,
      label: t(transitionLabelKeys[transition]),
      icon: transitionIcons[transition],
      isPending: pendingId === transition,
      onAction: () => void run(transition, () => actions.transition(item, transition)),
    }
  }

  const snoozeInstants = getSnoozePresetInstants(options.now, getLocalTimeZone())
  const snoozeChoices: PageAction[] = SNOOZE_PRESETS.map((preset) => ({
    id: `snooze-${preset}`,
    label: t(snoozePresetLabelKeys[preset]),
    icon: snoozePresetIcons[preset],
    isPending: pendingId === `snooze-${preset}`,
    onAction: () => void run(`snooze-${preset}`, () => actions.snooze(item, snoozeInstants[preset])),
  }))
  snoozeChoices.push({
    id: 'snooze-pickDate',
    label: t('snooze.pickDate'),
    icon: LuCalendarDays,
    onAction: options.onPickSnoozeDate,
  })
  if (isSnoozed(item, options.now)) {
    snoozeChoices.push({
      id: 'snooze-end',
      label: t('snooze.end'),
      icon: LuAlarmClockOff,
      isPending: pendingId === 'snooze-end',
      onAction: () => void run('snooze-end', () => actions.snooze(item, null)),
    })
  }

  const menu: PageAction[] = otherTransitions.map(transitionAction)
  if (!isClosed) {
    menu.push(
      { id: 'snooze', label: t('snooze.label'), icon: LuAlarmClock, items: snoozeChoices },
      {
        id: 'waitOn',
        label: item.waitingOn
          ? t('wait.change')
          : t('wait.action'),
        icon: LuHourglass,
        onAction: () => actions.waitOn(item),
      },
    )
  }
  if (item.waitingOn) {
    menu.push({
      id: 'stopWaiting',
      label: t('wait.stop'),
      icon: LuTimerOff,
      isPending: pendingId === 'stopWaiting',
      onAction: () => void run('stopWaiting', () => actions.stopWaiting(item)),
    })
  }
  menu.push(
    {
      id: 'startSession',
      label: t('sessions.start'),
      icon: LuSquareTerminal,
      href: getNewCodingSessionUrl(item.id),
    },
    {
      id: 'delete',
      label: t('common:actions.delete'),
      icon: LuTrash2,
      isDanger: true,
      isPending: pendingId === 'delete',
      // At once, with Undo: the page leaves for the list, and Undo comes back to it.
      onAction: () => void run('delete', () => actions.remove(item, UrlTree.actionItemsAll)),
    },
  )

  return {
    lead: leadTransition
      ? transitionAction(leadTransition)
      : null,
    actions: menu,
  } as const
}
