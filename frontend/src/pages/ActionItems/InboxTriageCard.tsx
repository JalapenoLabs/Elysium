// Copyright © 2026 Jalapeno Labs

import type { ActionItem, ActionItemTransition } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useNavigate } from 'react-router'

// User interface
import { Button, Card, Link } from '@heroui/react'
import { LuCheck, LuCircleX } from 'react-icons/lu'
import { ActionItemBadges } from './ActionItemBadges'
import { ActionItemMemberships } from './ActionItemMemberships'
import { ShortcutHint } from './ShortcutHint'

// Misc
import { getActionItemViewUrl } from '../../urls'
import { useActionItemActions } from './useActionItemActions'
import { useQuickActionHotkey } from './useQuickActionHotkey'

const SHORTCUTS = {
  accept: 'a',
  dismiss: 'd',
  open: 'o',
} as const

type Props = {
  item: ActionItem
  now: number
}

// One inbox item and the two decisions triage makes: accept it into Next, or dismiss it.
export function InboxTriageCard(props: Props) {
  const { t, i18n } = useTranslation('actionItems')
  const navigate = useNavigate()
  const actions = useActionItemActions()
  const [ isActing, setIsActing ] = useState(false)
  const item = props.item
  const dateFormatter = new Intl.DateTimeFormat(i18n.language, { dateStyle: 'medium', timeStyle: 'short' })

  async function decide(transition: ActionItemTransition) {
    if (isActing) {
      return
    }
    setIsActing(true)
    try {
      await actions.transition(item, transition)
    }
    finally {
      setIsActing(false)
    }
  }

  useQuickActionHotkey(SHORTCUTS.accept, () => void decide('accept'), !isActing)
  useQuickActionHotkey(SHORTCUTS.dismiss, () => void decide('dismiss'), !isActing)
  useQuickActionHotkey(SHORTCUTS.open, () => navigate(getActionItemViewUrl(item.id)), true)

  return <Card className='relaxed'>
    <Card.Header>
      <p className='text-xs opacity-60'>{
        t('inbox.arrived', { date: dateFormatter.format(new Date(item.createdAt)) })
      }</p>
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
      <Button onPress={() => void decide('accept')} isDisabled={isActing}>
        <LuCheck className='size-4' aria-hidden />
        <span>{t('transitions.accept')}</span>
        <ShortcutHint shortcut={SHORTCUTS.accept} />
      </Button>
      <Button variant='outline' onPress={() => void decide('dismiss')} isDisabled={isActing}>
        <LuCircleX className='size-4' aria-hidden />
        <span>{t('transitions.dismiss')}</span>
        <ShortcutHint shortcut={SHORTCUTS.dismiss} />
      </Button>
    </Card.Footer>
  </Card>
}
