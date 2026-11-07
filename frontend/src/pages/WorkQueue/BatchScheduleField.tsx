// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Calendar, DateField, DatePicker, FieldError, Label } from '@heroui/react'

// Utility
import { parseAbsoluteToLocal } from '@internationalized/date'

type Props = {
  // The instant it is due, with its offset, or null before one is picked.
  value: string | null
  errorMessage?: string
  onChange: (value: string | null) => void
}

// When a scheduled batch is due, picked to the minute in the viewer's own time zone and kept as
// the instant it names, which is how the API takes it.
export function BatchScheduleField(props: Props) {
  const { t } = useTranslation('workQueue')
  const value = props.value
    ? parseAbsoluteToLocal(props.value)
    : null

  return <DatePicker
    id='work-batch-scheduled-for'
    granularity='minute'
    isInvalid={Boolean(props.errorMessage)}
    value={value}
    onChange={(picked) => props.onChange(picked?.toAbsoluteString() ?? null)}
  >
    <Label>{t('edit.scheduledFor')}</Label>
    <DateField.Group fullWidth>
      <DateField.Input>{
        (segment) => <DateField.Segment segment={segment} />
      }</DateField.Input>
      <DateField.Suffix>
        <DatePicker.Trigger>
          <DatePicker.TriggerIndicator />
        </DatePicker.Trigger>
      </DateField.Suffix>
    </DateField.Group>
    <FieldError>{props.errorMessage}</FieldError>
    <DatePicker.Popover>
      <Calendar aria-label={t('edit.scheduledForPicker')}>
        <Calendar.Header>
          <Calendar.YearPickerTrigger>
            <Calendar.YearPickerTriggerHeading />
            <Calendar.YearPickerTriggerIndicator />
          </Calendar.YearPickerTrigger>
          <Calendar.NavButton slot='previous' />
          <Calendar.NavButton slot='next' />
        </Calendar.Header>
        <Calendar.Grid>
          <Calendar.GridHeader>{
            (day) => <Calendar.HeaderCell>{day}</Calendar.HeaderCell>
          }</Calendar.GridHeader>
          <Calendar.GridBody>{
            (date) => <Calendar.Cell date={date} />
          }</Calendar.GridBody>
        </Calendar.Grid>
        <Calendar.YearPickerGrid>
          <Calendar.YearPickerGridBody>{
            ({ year }) => <Calendar.YearPickerCell year={year} />
          }</Calendar.YearPickerGridBody>
        </Calendar.YearPickerGrid>
      </Calendar>
    </DatePicker.Popover>
  </DatePicker>
}
