// Copyright © 2026 Jalapeno Labs

import type { CalendarDate } from '@internationalized/date'
import type { ParseKeys } from 'i18next'
import type { Key } from '@heroui/react'
import type { ReactNode } from 'react'
import type { ActionItem } from '../../api/routes/actionItemRoutes'
import type { SnoozePreset } from './actionItemDates'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Dropdown, Label, Modal, useOverlayState } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'

// Utility
import { getLocalTimeZone, today } from '@internationalized/date'

// Misc
import { getSnoozePresetInstants, SNOOZE_PRESETS, toSnoozeInstant } from './actionItemDates'
import { isSnoozed } from './actionItemPresentation'

const presetLabelKeys = {
  laterToday: 'snooze.laterToday',
  tomorrow: 'snooze.tomorrow',
  nextWeek: 'snooze.nextWeek',
} as const satisfies Record<SnoozePreset, ParseKeys<'actionItems'>>

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
  // The trigger's content, so each view can show its own label and shortcut.
  children: ReactNode
  isDisabled?: boolean
  size?: 'sm' | 'md'
}

// Snoozes an item: a few hours, tomorrow morning, next Monday, or a day picked from a
// calendar. A snoozed item can have its snooze ended here too.
export function SnoozeMenu(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const dateDialog = useOverlayState()
  const [ pickedDate, setPickedDate ] = useState<CalendarDate | null>(null)
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
      setPickedDate(today(timeZone).add({ days: 1 }))
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

  async function saveDate() {
    if (!pickedDate) {
      console.debug('SnoozeMenu saved without a picked date')
      return
    }
    // A failure has said so in a toast; the dialog stays open to try again.
    const isSaved = await save(toSnoozeInstant(pickedDate, timeZone))
    if (isSaved) {
      dateDialog.close()
    }
  }

  return <>
    <Dropdown isOpen={props.isOpen} onOpenChange={props.onOpenChange}>
      <Button size={props.size} variant='outline' isDisabled={props.isDisabled} isPending={isSaving}>
        {props.children}
      </Button>
      <Dropdown.Popover placement='bottom start'>
        <Dropdown.Menu aria-label={t('snooze.label')} onAction={choose}>
          {SNOOZE_PRESETS.map((preset) => <Dropdown.Item
            key={preset}
            id={preset}
            textValue={t(presetLabelKeys[preset])}
          >
            <Label>{t(presetLabelKeys[preset])}</Label>
          </Dropdown.Item>)}
          <Dropdown.Item id={PICK_DATE} textValue={t('snooze.pickDate')}>
            <Label>{t('snooze.pickDate')}</Label>
          </Dropdown.Item>
          {isSnoozed(props.item, props.now)
            ? <Dropdown.Item id={END_SNOOZE} textValue={t('snooze.end')}>
              <Label>{t('snooze.end')}</Label>
            </Dropdown.Item>
            : null}
        </Dropdown.Menu>
      </Dropdown.Popover>
    </Dropdown>

    <Modal.Backdrop isOpen={dateDialog.isOpen} onOpenChange={dateDialog.setOpen}>
      <Modal.Container>
        <Modal.Dialog className='sm:max-w-sm'>
          <Modal.CloseTrigger />
          <Modal.Header>
            <Modal.Heading>{t('snooze.dialogTitle')}</Modal.Heading>
          </Modal.Header>
          <Modal.Body className='mt-2 mb-4'>
            <DayPicker
              label={t('snooze.dateLabel')}
              calendarLabel={t('snooze.dateLabel')}
              description={t('snooze.dateHint')}
              value={pickedDate}
              minValue={today(timeZone).add({ days: 1 })}
              onChange={setPickedDate}
            />
          </Modal.Body>
          <Modal.Footer>
            <Button variant='tertiary' onPress={dateDialog.close}>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button isDisabled={!pickedDate} isPending={isSaving} onPress={() => void saveDate()}>
              <span>{t('snooze.submit')}</span>
            </Button>
          </Modal.Footer>
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  </>
}
