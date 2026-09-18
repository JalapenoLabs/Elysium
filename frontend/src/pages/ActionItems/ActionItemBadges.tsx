// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Chip } from '@heroui/react'
import { LuAlarmClock, LuCalendar, LuHourglass } from 'react-icons/lu'

// Misc
import {
  describeOwner,
  isOverdue,
  isSnoozed,
  priorityChipColors,
  priorityLabelKeys,
  stateChipColors,
  stateLabelKeys,
} from './actionItemPresentation'

type Props = {
  item: ActionItem
  // Milliseconds, from useNow, for overdue and snoozed.
  now: number
  // Next shows only open items, so it leaves the state out.
  showState?: boolean
}

// The facts about an item worth seeing at a glance: its state, a priority other than
// normal, when it is due (in red once overdue), a snooze, who it waits on, and whose it is
// when it is not the user's.
export function ActionItemBadges(props: Props) {
  const { t, i18n } = useTranslation('actionItems')
  const item = props.item
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium' })
  const dateTimeFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })
  const owner = describeOwner(item)

  return <div className='flex flex-wrap items-center gap-2'>
    {props.showState && <Chip size='sm' variant='soft' color={stateChipColors[item.state]}>{
      t(stateLabelKeys[item.state])
    }</Chip>}
    {item.priority !== 'normal' && <Chip size='sm' variant='soft' color={priorityChipColors[item.priority]}>{
      t(priorityLabelKeys[item.priority])
    }</Chip>}
    {item.dueAt && <Chip
      size='sm'
      variant='soft'
      color={isOverdue(item, props.now)
        ? 'danger'
        : 'default'}
    >
      <LuCalendar className='size-3' aria-hidden />
      <span>{isOverdue(item, props.now)
        ? t('badges.overdue', { date: dateFormatter.format(new Date(item.dueAt)) })
        : t('badges.due', { date: dateFormatter.format(new Date(item.dueAt)) })}</span>
    </Chip>}
    {item.snoozedUntil && isSnoozed(item, props.now) && <Chip size='sm' variant='soft'>
      <LuAlarmClock className='size-3' aria-hidden />
      <span>{t('badges.snoozed', { date: dateTimeFormatter.format(new Date(item.snoozedUntil)) })}</span>
    </Chip>}
    {item.waitingOn && <Chip size='sm' variant='soft' color='warning'>
      <LuHourglass className='size-3' aria-hidden />
      <span>{t('badges.waiting', { name: item.waitingOn })}</span>
    </Chip>}
    {item.owner.kind !== 'user' && <Chip size='sm' variant='soft'>{
      t('badges.owner', { owner: t(owner.key, owner.values) })
    }</Chip>}
  </div>
}
