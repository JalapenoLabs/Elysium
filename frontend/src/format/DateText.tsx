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
  // Styles the trigger, the element that sits in the surrounding layout.
  className?: string
}

// A date in its short form, "January 1st", for tables and lists, with the full timestamp in a
// tooltip. The tooltip's trigger takes focus, so the full timestamp reaches the keyboard too. It
// renders a `div`, so it belongs in a `div` or `span` parent, never a `p`.
export function DateText(props: Props) {
  const date = new Date(props.value)

  return <Tooltip delay={400}>
    <Tooltip.Trigger className={`inline ${props.className ?? ''}`}>
      <time
        dateTime={Number.isNaN(date.getTime())
          ? undefined
          : date.toISOString()}
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
