// Copyright © 2026 Jalapeno Labs

// Core
import { useTranslation } from 'react-i18next'

// User interface
import { Calendar, DateField, DatePicker, FieldError, Label } from '@heroui/react'

// Utility
import { parseAbsoluteToLocal } from '@internationalized/date'

type Props = {
  // The instant it started, with its offset, or null before one is picked.
  value: string | null
  errorMessage?: string
  isDisabled: boolean
  onChange: (value: string | null) => void
}

// When an uploaded meeting started, picked to the minute in the viewer's own time zone and kept
// as the instant it names, which is how the API takes it.
export function MeetingStartField(props: Props) {
  const { t } = useTranslation('meetings')
  const value = props.value
    ? parseAbsoluteToLocal(props.value)
    : null

  return <DatePicker
    id='meeting-upload-started-at'
    granularity='minute'
    isRequired
    isDisabled={props.isDisabled}
    isInvalid={Boolean(props.errorMessage)}
    value={value}
    onChange={(picked) => props.onChange(picked?.toAbsoluteString() ?? null)}
  >
    <Label>{t('upload.fields.startedAt')}</Label>
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
      <Calendar aria-label={t('upload.fields.startedAtPicker')}>
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
