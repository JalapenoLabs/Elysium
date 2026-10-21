// Copyright © 2026 Jalapeno Labs

import type { AvailabilityWindow } from '../api/routes/elysiaRoutes'
import type { Weekday } from './hours'

// Core
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

// Redux
import { useAppDispatch } from '../store/hooks'
import { elysiaUpdated } from '../store/elysiaSlice'

// User interface
import { Button, Checkbox, Label, Modal, Switch, TimeField, toast } from '@heroui/react'

// Utility
import { Time } from '@internationalized/date'

// Misc
import { updateElysiaSettings } from '../api/routes/elysiaRoutes'
import { WEEKDAYS, fromUtcWindows, localOffsetMinutes, toUtcWindows } from './hours'

// A new schedule starts as working hours, Monday to Friday.
const DEFAULT_DAYS: Weekday[] = [ 0, 1, 2, 3, 4 ]
const DEFAULT_START = new Time(9, 0)
const DEFAULT_END = new Time(17, 0)

type Props = {
  isOpen: boolean
  onOpenChange: (isOpen: boolean) => void
  availability: AvailabilityWindow[]
}

// Edits the hours Elysia may start work on her own: one daily range on chosen days, in the
// viewer's zone. A person's own messages are answered whatever the hours say.
export function ElysiaHoursDialog(props: Props) {
  const { t, i18n } = useTranslation('elysia')
  const dispatch = useAppDispatch()
  const offset = localOffsetMinutes()
  const saved = fromUtcWindows(props.availability, offset)
  const [ isAnyTime, setIsAnyTime ] = useState(!props.availability.length)
  const [ days, setDays ] = useState<Weekday[]>(saved?.days ?? DEFAULT_DAYS)
  const [ start, setStart ] = useState<Time>(
    saved
      ? new Time(Math.floor(saved.startMinute / 60), saved.startMinute % 60)
      : DEFAULT_START,
  )
  const [ end, setEnd ] = useState<Time>(
    saved
      ? new Time(Math.floor(saved.endMinute / 60), saved.endMinute % 60)
      : DEFAULT_END,
  )
  const [ isSaving, setIsSaving ] = useState(false)

  const startMinute = start.hour * 60 + start.minute
  const endMinute = end.hour * 60 + end.minute
  const isValid = isAnyTime || (Boolean(days.length) && startMinute < endMinute)
  const dayFormatter = new Intl.DateTimeFormat(i18n.language, { weekday: 'short' })

  async function save() {
    const availability = isAnyTime
      ? []
      : toUtcWindows({ days, startMinute, endMinute }, offset)
    setIsSaving(true)
    try {
      const response = await updateElysiaSettings({ availability })
      dispatch(elysiaUpdated(response.elysia))
      props.onOpenChange(false)
    }
    catch (error) {
      console.debug('ElysiaHoursDialog failed to save her hours', { error, availability })
      toast.danger(t('panel.settingsFailed'))
    }
    finally {
      setIsSaving(false)
    }
  }

  return <Modal.Backdrop isOpen={props.isOpen} onOpenChange={props.onOpenChange}>
    <Modal.Container>
      <Modal.Dialog className='sm:max-w-md'>
        <Modal.CloseTrigger />
        <Modal.Header>
          <Modal.Heading>{t('hours.title')}</Modal.Heading>
        </Modal.Header>
        <Modal.Body className='mt-2 flex flex-col gap-4'>
          <p className='text-sm opacity-80'>{t('hours.description')}</p>
          {Boolean(props.availability.length) && !saved && <p className='text-sm text-warning'>{
            t('hours.custom')
          }</p>}
          <Switch isSelected={isAnyTime} onChange={setIsAnyTime}>
            <Switch.Control>
              <Switch.Thumb />
            </Switch.Control>
            <span className='text-sm'>{t('hours.anyTime')}</span>
          </Switch>
          {!isAnyTime && <>
            <fieldset>
              <legend className='compact text-sm font-medium'>{t('hours.days')}</legend>
              <div className='flex flex-wrap gap-3'>{
                WEEKDAYS.map((day) => <Checkbox
                  key={day}
                  isSelected={days.includes(day)}
                  onChange={(isSelected) => setDays((current) => isSelected
                    ? [ ...current, day ]
                    : current.filter((existing) => existing !== day))}
                >
                  <Checkbox.Content>
                    <Checkbox.Control>
                      <Checkbox.Indicator />
                    </Checkbox.Control>
                    <span className='text-sm'>{
                      // 2026-10-19 is a Monday, so day 0 lands on it.
                      dayFormatter.format(new Date(2026, 9, 19 + day))
                    }</span>
                  </Checkbox.Content>
                </Checkbox>)
              }</div>
            </fieldset>
            <div className='level gap-3'>
              <TimeField className='w-full' value={start} onChange={(value) => value && setStart(value)}>
                <Label>{t('hours.from')}</Label>
                <TimeField.Group>
                  <TimeField.Input>{(segment) => <TimeField.Segment segment={segment} />}</TimeField.Input>
                </TimeField.Group>
              </TimeField>
              <TimeField className='w-full' value={end} onChange={(value) => value && setEnd(value)}>
                <Label>{t('hours.until')}</Label>
                <TimeField.Group>
                  <TimeField.Input>{(segment) => <TimeField.Segment segment={segment} />}</TimeField.Input>
                </TimeField.Group>
              </TimeField>
            </div>
            <p className='text-xs opacity-60'>{t('hours.daylightSaving')}</p>
          </>}
        </Modal.Body>
        <Modal.Footer>
          <Button variant='tertiary' isDisabled={isSaving} onPress={() => props.onOpenChange(false)}>
            <span>{t('hours.cancel')}</span>
          </Button>
          <Button isDisabled={!isValid} isPending={isSaving} onPress={() => void save()}>
            <span>{t('hours.save')}</span>
          </Button>
        </Modal.Footer>
      </Modal.Dialog>
    </Modal.Container>
  </Modal.Backdrop>
}
