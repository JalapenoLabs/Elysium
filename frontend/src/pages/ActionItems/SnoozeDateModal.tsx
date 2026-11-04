// Copyright © 2026 Jalapeno Labs

import type { CalendarDate } from '@internationalized/date'
import type { UseOverlayStateReturn } from '@heroui/react'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// User interface
import { Button, Form, Modal } from '@heroui/react'
import { DayPicker } from '../../components/DayPicker'

// Utility
import { getLocalTimeZone, today } from '@internationalized/date'

// Misc
import { submitOnEnter } from '../../components/submitOnEnter'
import { toSnoozeInstant } from './actionItemDates'

type Props = {
  state: UseOverlayStateReturn
  // Resolves to whether the snooze was saved. A failure has said so in a toast, and the
  // dialog stays open to try again.
  onSnooze: (until: string) => Promise<boolean>
}

// Snoozes an item until a day picked from a calendar, which wakes it in the morning. Mount it
// only while open, so every opening starts on tomorrow.
export function SnoozeDateModal(props: Props) {
  const { t } = useTranslation([ 'actionItems', 'common' ])
  const timeZone = getLocalTimeZone()
  const [ pickedDate, setPickedDate ] = useState<CalendarDate | null>(() => today(timeZone).add({ days: 1 }))
  const [ isSaving, setIsSaving ] = useState(false)

  async function save() {
    if (!pickedDate || isSaving) {
      console.debug('SnoozeDateModal ignored a save', { hasDate: Boolean(pickedDate), isSaving })
      return
    }
    setIsSaving(true)
    try {
      const isSaved = await props.onSnooze(toSnoozeInstant(pickedDate, timeZone))
      if (isSaved) {
        props.state.close()
      }
    }
    finally {
      setIsSaving(false)
    }
  }

  return <Modal.Backdrop isOpen={props.state.isOpen} onOpenChange={props.state.setOpen}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-sm'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('snooze.dialogTitle')}</Modal.Heading>
        </Modal.Header>
        {/* Enter in the date's segments snoozes (ux.md, Hotkeys); the calendar's own Enter picks a day */}
        <Form
          validationBehavior='aria'
          onKeyDownCapture={submitOnEnter}
          onSubmit={(event) => {
            event.preventDefault()
            void save()
          }}
        >
          <Modal.Body className='mt-2 mb-4'>
            <DayPicker
              id='snooze-date'
              label={t('snooze.dateLabel')}
              calendarLabel={t('snooze.dateLabel')}
              description={t('snooze.dateHint')}
              value={pickedDate}
              minValue={today(timeZone).add({ days: 1 })}
              onChange={setPickedDate}
            />
          </Modal.Body>
          <Modal.Footer>
            <Button variant='tertiary' onPress={props.state.close}>
              <span>{t('common:actions.cancel')}</span>
            </Button>
            <Button type='submit' isDisabled={!pickedDate} isPending={isSaving}>
              <span>{t('snooze.submit')}</span>
            </Button>
          </Modal.Footer>
        </Form>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
