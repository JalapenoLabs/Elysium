// Copyright © 2026 Jalapeno Labs

import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// User interface
import { Button, Card, Link } from '@heroui/react'
import {
  LuAlarmClock,
  LuCircleCheck,
  LuCircleX,
  LuHourglass,
  LuMessageSquare,
  LuSkipForward,
  LuSquareTerminal,
} from 'react-icons/lu'
import { ActionItemBadges } from './ActionItemBadges'
import { ActionItemComments } from './ActionItemComments'
import { ActionItemHistory } from './ActionItemHistory'
import { ActionItemMemberships } from './ActionItemMemberships'
import { ShortcutHint } from './ShortcutHint'
import { SnoozeMenu } from './SnoozeMenu'

// Misc
import { getActionItemViewUrl, getNewCodingSessionUrl } from '../../urls'
import { useActionItemActions } from './useActionItemActions'
import { useQuickActionHotkey } from './useQuickActionHotkey'

// The keys for Next's quick actions, shown on their buttons.
const SHORTCUTS = {
  resolve: 'r',
  dismiss: 'd',
  snooze: 's',
  wait: 'w',
  comment: 'c',
  // G for "go": put an agent on it.
  code: 'g',
  skip: 'j',
  open: 'o',
} as const

type Props = {
  item: ActionItem
  // Where the item stands in Next, from 1.
  position: number
  total: number
  now: number
  onSkip: () => void
}

// One item from Next with everything needed to act on it: its facts, notes, projects and
// initiatives, conversation, and history, and a row of quick actions, each with a key.
export function NextItemCard(props: Props) {
  const { t } = useTranslation('actionItems')
  const navigate = useNavigate()
  const actions = useActionItemActions()
  const composerRef = useRef<HTMLTextAreaElement>(null)
  const [ isSnoozeOpen, setIsSnoozeOpen ] = useState(false)
  const [ isActing, setIsActing ] = useState(false)
  const item = props.item

  // Resolving or dismissing takes the item out of Next, which swaps this card for the next
  // one. Presses while one is in flight are ignored.
  async function act(action: () => Promise<boolean>) {
    if (isActing) {
      return
    }
    setIsActing(true)
    try {
      await action()
    }
    finally {
      setIsActing(false)
    }
  }

  const resolve = () => void act(() => actions.transition(item, 'resolve'))
  const dismiss = () => void act(() => actions.transition(item, 'dismiss'))
  const waitOn = () => actions.waitOn(item)
  const focusComposer = () => composerRef.current?.focus()
  const open = () => navigate(getActionItemViewUrl(item.id))
  const startSession = () => navigate(getNewCodingSessionUrl(item.id))

  useQuickActionHotkey(SHORTCUTS.resolve, resolve, !isActing)
  useQuickActionHotkey(SHORTCUTS.dismiss, dismiss, !isActing)
  useQuickActionHotkey(SHORTCUTS.snooze, () => setIsSnoozeOpen(true), !isActing)
  useQuickActionHotkey(SHORTCUTS.wait, waitOn, !isActing)
  useQuickActionHotkey(SHORTCUTS.comment, focusComposer, true)
  useQuickActionHotkey(SHORTCUTS.code, startSession, true)
  useQuickActionHotkey(SHORTCUTS.skip, props.onSkip, props.total > 1)
  useQuickActionHotkey(SHORTCUTS.open, open, true)

  return <div>
    <Card className='relaxed'>
      <Card.Header>
        <div className='level compact text-xs opacity-60'>
          <span>{t('next.position', { position: props.position, total: props.total })}</span>
        </div>
        <Card.Title className='text-2xl font-bold'>
          <Link href={getActionItemViewUrl(item.id)} className='text-inherit no-underline hover:underline'>{
            item.title
          }</Link>
        </Card.Title>
      </Card.Header>
      <Card.Content className='flex flex-col gap-4'>
        <ActionItemBadges item={item} now={props.now} />
        {item.notes && <p className='text-sm whitespace-pre-line opacity-90'>{item.notes}</p>}
        <ActionItemMemberships item={item} />
      </Card.Content>
      <Card.Footer className='flex flex-wrap gap-2'>
        <Button onPress={resolve} isDisabled={isActing}>
          <LuCircleCheck className='size-4' aria-hidden />
          <span>{t('transitions.resolve')}</span>
          <ShortcutHint shortcut={SHORTCUTS.resolve} />
        </Button>
        <Button variant='outline' onPress={dismiss} isDisabled={isActing}>
          <LuCircleX className='size-4' aria-hidden />
          <span>{t('transitions.dismiss')}</span>
          <ShortcutHint shortcut={SHORTCUTS.dismiss} />
        </Button>
        <SnoozeMenu
          item={item}
          now={props.now}
          isOpen={isSnoozeOpen}
          onOpenChange={setIsSnoozeOpen}
          onSnooze={(until) => actions.snooze(item, until)}
          isDisabled={isActing}
        >
          <LuAlarmClock className='size-4' aria-hidden />
          <span>{t('snooze.label')}</span>
          <ShortcutHint shortcut={SHORTCUTS.snooze} />
        </SnoozeMenu>
        <Button variant='outline' onPress={waitOn} isDisabled={isActing}>
          <LuHourglass className='size-4' aria-hidden />
          <span>{t('wait.action')}</span>
          <ShortcutHint shortcut={SHORTCUTS.wait} />
        </Button>
        <Button variant='ghost' onPress={focusComposer}>
          <LuMessageSquare className='size-4' aria-hidden />
          <span>{t('comments.action')}</span>
          <ShortcutHint shortcut={SHORTCUTS.comment} />
        </Button>
        <Button variant='ghost' onPress={startSession}>
          <LuSquareTerminal className='size-4' aria-hidden />
          <span>{t('sessions.start')}</span>
          <ShortcutHint shortcut={SHORTCUTS.code} />
        </Button>
        <Button variant='ghost' onPress={props.onSkip} isDisabled={props.total < 2} className='ml-auto'>
          <LuSkipForward className='size-4' aria-hidden />
          <span>{t('next.skip')}</span>
          <ShortcutHint shortcut={SHORTCUTS.skip} />
        </Button>
      </Card.Footer>
    </Card>

    <div className='grid grid-cols-1 gap-8 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]'>
      <section>
        <h2 className='compact text-lg font-semibold'>{t('comments.heading')}</h2>
        <ActionItemComments item={item} composerRef={composerRef} />
      </section>
      <section>
        <h2 className='compact text-lg font-semibold'>{t('history.heading')}</h2>
        <ActionItemHistory itemId={item.id} />
      </section>
    </div>
  </div>
}
