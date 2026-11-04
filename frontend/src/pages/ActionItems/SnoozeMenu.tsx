// Copyright © 2026 Jalapeno Labs

import type { Key } from '@heroui/react'
import type { ReactNode } from 'react'
import type { ActionItem } from '../../api/routes/actionItemRoutes'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label, useOverlayState } from '@heroui/react'
import { LuAlarmClockOff, LuCalendarDays, LuChevronDown } from 'react-icons/lu'
import { SnoozeDateModal } from './SnoozeDateModal'

// Utility
import { getLocalTimeZone } from '@internationalized/date'

// Misc
import { getSnoozePresetInstants, SNOOZE_PRESETS } from './actionItemDates'
import { isSnoozed, snoozePresetIcons, snoozePresetLabelKeys } from './actionItemPresentation'

const PICK_DATE = 'pickDate'
const END_SNOOZE = 'end'

type Props = {
  item: ActionItem
  now: number
  // Resolves to whether the snooze was saved.
  onSnooze: (until: string | null) => Promise<boolean>
  // The menu can be opened from outside, such as by a keyboard shortcut.
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
  // The trigger's content, so each view can show its own label and shortcut. The chevron that
  // marks it a menu follows it.
  children: ReactNode
  isDisabled?: boolean
  size?: 'sm' | 'md'
}

// Snoozes an item: a few hours, tomorrow morning, next Monday, or a day picked from a
// calendar. A snoozed item can have its snooze ended here too.
export function SnoozeMenu(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dateDialog = useOverlayState()
  const [ isSaving, setIsSaving ] = useState(false)
  const timeZone = getLocalTimeZone()

  async function save(until: string | null) {
    setIsSaving(true)
    try {
      return await props.onSnooze(until)
    }
    finally {
      setIsSaving(false)
    }
  }

  function choose(key: Key) {
    const choice = String(key)
    if (choice === PICK_DATE) {
      dateDialog.open()
      return
    }
    if (choice === END_SNOOZE) {
      void save(null)
      return
    }
    const instants = getSnoozePresetInstants(props.now, timeZone)
    const preset = SNOOZE_PRESETS.find((name) => name === choice)
    if (!preset) {
      console.debug('SnoozeMenu received a choice it does not offer', { choice })
      return
    }
    void save(instants[preset])
  }

  return <>
    <Dropdown isOpen={props.isOpen} onOpenChange={props.onOpenChange}>
      <Button size={props.size} variant='outline' isDisabled={props.isDisabled} isPending={isSaving}>
        {props.children}
        <LuChevronDown className='size-4' aria-hidden />
      </Button>
      <Dropdown.Popover placement='bottom start'>
        <Dropdown.Menu aria-label={t('snooze.label')} onAction={choose}>
          {SNOOZE_PRESETS.map((preset) => {
            const Icon = snoozePresetIcons[preset]
            return <Dropdown.Item
              key={preset}
              id={preset}
              textValue={t(snoozePresetLabelKeys[preset])}
            >
              <Icon className='size-4' aria-hidden />
              <Label>{t(snoozePresetLabelKeys[preset])}</Label>
            </Dropdown.Item>
          })}
          <Dropdown.Item id={PICK_DATE} textValue={t('snooze.pickDate')}>
            <LuCalendarDays className='size-4' aria-hidden />
            <Label>{t('snooze.pickDate')}</Label>
          </Dropdown.Item>
          {isSnoozed(props.item, props.now)
            ? <Dropdown.Item id={END_SNOOZE} textValue={t('snooze.end')}>
              <LuAlarmClockOff className='size-4' aria-hidden />
              <Label>{t('snooze.end')}</Label>
            </Dropdown.Item>
            : null}
        </Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>

    {dateDialog.isOpen && <SnoozeDateModal
      state={dateDialog}
      onSnooze={save}
    />}
  </>
}
