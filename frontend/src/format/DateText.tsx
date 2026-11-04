// Copyright © 2026 Jalapeno Labs

import type { DateInput } from './dates'

// User interface
import { Tooltip } from '@heroui/react'

// Misc
import { formatDate, formatDateTime, formatShortDate } from './dates'

type Props = {
  value: DateInput
  // A day rather than a moment, such as a due date, whose time of day is only the day's end:
  // the tooltip leaves the time out.
  isDay?: boolean
  className?: string
}

// A date in its short form, "January 1st", for tables and lists, with the full timestamp in a
// tooltip. The tooltip's trigger takes focus, so the full timestamp reaches the keyboard too.
export function DateText(props: Props) {
  const date = new Date(props.value)

  return <Tooltip delay={400}>
    <Tooltip.Trigger className='inline'>
      <time
        dateTime={Number.isNaN(date.getTime())
          ? undefined
          : date.toISOString()}
        className={props.className}
      >{
          formatShortDate(props.value)
        }</time>
    </Tooltip.Trigger>
    <Tooltip.Content>{
      props.isDay
        ? formatDate(props.value)
        : formatDateTime(props.value)
    }</Tooltip.Content>
  </Tooltip>
}
