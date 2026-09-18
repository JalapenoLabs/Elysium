// Copyright © 2026 Jalapeno Labs

import type { CalendarDate } from '@internationalized/date'

// User interface
import { Calendar, DateField, DatePicker, Description, Label } from '@heroui/react'

type Props = {
  label: string
  // Names the calendar popover for screen readers.
  calendarLabel: string
  value: CalendarDate | null
  onChange: (value: CalendarDate | null) => void
  description?: string
  // The earliest day that can be picked.
  minValue?: CalendarDate
  isDisabled?: boolean
}

// A date without a time: typed segments plus a calendar, in the viewer's locale. Callers
// turn the day into an instant (see actionItemDates.ts), since what a day means differs:
// a due date lasts until the day ends, a snooze wakes that morning.
export function DayPicker(props: Props) {
  return <DatePicker
    granularity='day'
    value={props.value}
    minValue={props.minValue}
    isDisabled={props.isDisabled}
    onChange={props.onChange}
  >
    <Label>{props.label}</Label>
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
    {props.description && <Description>{props.description}</Description>}
    <DatePicker.Popover>
      <Calendar aria-label={props.calendarLabel}>
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
